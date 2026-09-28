//! A node's side of the cluster: announcing itself, and recording what it
//! runs.
//!
//! The node is the authority on its own sandboxes. It writes a record when it
//! creates one and deletes it when the sandbox ends -- by request, by timeout,
//! or by the node restarting -- so the store says what the node is actually
//! running rather than what a control plane last asked for.

use std::sync::Arc;
use std::time::Duration;

use crate::model::{now_ms, ClusterEvent, NodeInfo, SandboxRecord};
use crate::store::ClusterStore;

/// How a node joins a cluster.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub id: String,
    pub api: String,
    pub proxy: std::net::SocketAddr,
    pub capacity: u32,
    /// How long the node's record lives without a heartbeat. Heartbeats go
    /// out at a third of this, so one lost heartbeat is not a death.
    pub ttl: Duration,
    /// Its workload-token signing key, public half, as a JWK.
    pub jwk: Option<serde_json::Value>,
    /// The templates it offers.
    pub templates: Vec<String>,
}

/// A node's handle on the cluster store.
#[derive(Clone)]
pub struct NodeAgent {
    store: Arc<dyn ClusterStore>,
    config: NodeConfig,
    /// The templates it offers now: set at start from the config, and
    /// again as templates are built while it runs.
    templates: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl NodeAgent {
    #[must_use]
    pub fn new(store: Arc<dyn ClusterStore>, config: NodeConfig) -> Self {
        let templates = Arc::new(parking_lot::Mutex::new(config.templates.clone()));
        Self {
            store,
            config,
            templates,
        }
    }

    /// Offer these templates from the next heartbeat on.
    pub fn set_templates(&self, templates: Vec<String>) {
        *self.templates.lock() = templates;
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.config.id
    }

    fn info(&self, running: u32) -> NodeInfo {
        NodeInfo {
            id: self.config.id.clone(),
            api: self.config.api.clone(),
            proxy: self.config.proxy,
            capacity: self.config.capacity,
            running,
            heartbeat_ms: now_ms(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            jwk: self.config.jwk.clone(),
            templates: self.templates.lock().clone(),
        }
    }

    /// Join: forget any sandboxes the store says this node was running --
    /// they died with the process that ran them, unless paused into shared
    /// storage -- and announce the node.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn join(&self) -> crate::store::Result<()> {
        for record in self.store.sandboxes().await? {
            // Paused into shared storage, it did not die with the process.
            if record.node_id == self.config.id
                && !record.survives_its_node()
                && self.store.delete_sandbox(&record.sandbox_id).await?
            {
                self.store
                    .publish(
                        &ClusterEvent::new(
                            "sandbox-lost",
                            &self.config.id,
                            Some(&record.sandbox_id),
                        )
                        .with_detail("the node restarted"),
                    )
                    .await?;
            }
        }
        self.store.put_node(&self.info(0), self.config.ttl).await?;
        self.store
            .publish(&ClusterEvent::new("node-joined", &self.config.id, None))
            .await
    }

    /// Heartbeat forever, reporting `running()` each time.
    ///
    /// A failed heartbeat is logged and retried at the next interval rather
    /// than ending the loop: a store that is briefly unreachable should cost
    /// the node its liveness for that long, not for good.
    pub async fn heartbeat(self, running: impl Fn() -> u32 + Send + 'static) {
        let interval = self.config.ttl / 3;
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = self
                .store
                .put_node(&self.info(running()), self.config.ttl)
                .await
            {
                tracing::warn!("cluster heartbeat for node {}: {e}", self.config.id);
            }
        }
    }

    /// Announce the node now, with `running` sandboxes.
    ///
    /// Called on every change as well as on the heartbeat, because the
    /// scheduler reads load from here: a node that reported its load only on
    /// the heartbeat looked empty for up to a third of its TTL after each
    /// create, and a burst of creations all landed on it.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn announce(&self, running: u32) -> crate::store::Result<()> {
        self.store
            .put_node(&self.info(running), self.config.ttl)
            .await
    }

    /// Record a sandbox this node now runs, and that it now runs `running`.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn created(
        &self,
        record: &SandboxRecord,
        running: u32,
    ) -> crate::store::Result<ClusterEvent> {
        self.store.put_sandbox(record).await?;
        self.announce(running).await?;
        let event = ClusterEvent::new("sandbox-created", &self.config.id, Some(&record.sandbox_id))
            .with_template(&record.template_id);
        self.store.publish(&event).await?;
        Ok(event)
    }

    /// Update a sandbox's record, e.g. its end time after a timeout change.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn updated(&self, record: &SandboxRecord) -> crate::store::Result<()> {
        self.store.put_sandbox(record).await
    }

    /// Record a sandbox that paused or resumed (`kind` is `sandbox-paused` or
    /// `sandbox-resumed`), leaving `running`: it stays this node's either
    /// way, since its snapshot is on this node's disk.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn transitioned(
        &self,
        record: &SandboxRecord,
        kind: &str,
        running: u32,
    ) -> crate::store::Result<ClusterEvent> {
        self.store.put_sandbox(record).await?;
        self.announce(running).await?;
        let event = ClusterEvent::new(kind, &self.config.id, Some(&record.sandbox_id))
            .with_template(&record.template_id);
        self.store.publish(&event).await?;
        Ok(event)
    }

    /// Record that a sandbox ended, and why (`sandbox-deleted` or
    /// `sandbox-expired`), leaving `running`.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn ended(
        &self,
        sandbox_id: &str,
        template_id: Option<&str>,
        kind: &str,
        running: u32,
    ) -> crate::store::Result<Option<ClusterEvent>> {
        self.announce(running).await?;
        if !self.store.delete_sandbox(sandbox_id).await? {
            return Ok(None);
        }
        let mut event = ClusterEvent::new(kind, &self.config.id, Some(sandbox_id));
        if let Some(template) = template_id {
            event = event.with_template(template);
        }
        self.store.publish(&event).await?;
        Ok(Some(event))
    }

    /// The store this node records into.
    #[must_use]
    pub fn store(&self) -> &Arc<dyn ClusterStore> {
        &self.store
    }

    /// Leave cleanly: the node's record goes now rather than at its TTL.
    ///
    /// # Errors
    ///
    /// The store could not be reached.
    pub async fn leave(&self) -> crate::store::Result<()> {
        self.store.remove_node(&self.config.id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::sandbox;
    use crate::store::MemoryStore;

    fn agent(store: Arc<dyn ClusterStore>) -> NodeAgent {
        NodeAgent::new(
            store,
            NodeConfig {
                id: "a".into(),
                api: "http://a:3980".into(),
                proxy: "127.0.0.1:3981".parse().unwrap(),
                capacity: 4,
                ttl: Duration::from_secs(9),
                jwk: None,
                templates: Vec::new(),
            },
        )
    }

    /// A node that restarts lost every VM it had; the store must not go on
    /// routing to them.
    #[tokio::test]
    async fn joining_forgets_what_a_previous_run_left_behind() {
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
        store.put_sandbox(&sandbox("old", "a")).await.unwrap();
        store.put_sandbox(&sandbox("theirs", "b")).await.unwrap();
        agent(Arc::clone(&store)).join().await.unwrap();

        assert!(store.sandbox("old").await.unwrap().is_none());
        assert!(store.sandbox("theirs").await.unwrap().is_some(), "not ours");
        assert_eq!(store.nodes().await.unwrap().len(), 1);
        let kinds: Vec<_> = store
            .events(10)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, ["node-joined", "sandbox-lost"]);
    }

    #[tokio::test]
    async fn an_end_is_reported_once() {
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
        let a = agent(Arc::clone(&store));
        a.created(&sandbox("s", "a"), 1).await.unwrap();
        a.ended("s", None, "sandbox-expired", 0).await.unwrap();
        a.ended("s", None, "sandbox-deleted", 0).await.unwrap();
        let kinds: Vec<_> = store
            .events(10)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, ["sandbox-expired", "sandbox-created"]);
    }
}
