//! The capabilities that group the machine, fork, snapshot and checkpoint
//! operations, for the control plane and for the node. The operations are in
//! `machine_operations` and `checkpoint_operations`.

use super::spec::capability;
use super::{Capability, HyperMachineOntology};

impl HyperMachineOntology {
    /// The machine capabilities of the control plane (`node` false) or the
    /// node: the same ids, which the node's carry `node_` before.
    pub(super) fn build_machine_capabilities(node: bool) -> Vec<Capability> {
        let (who, auth, perms, running): (&str, &str, &[&str], &str) = if node {
            (
                "the node daemon",
                "It has no users or teams: it checks only the cluster token when it has \
                 one and otherwise answers every caller that can reach it, and the team it \
                 acts for is whichever the control plane names in a header. It answers 501 \
                 on a host that is not Linux with KVM.",
                &["cluster token"],
                "a node (`hv2-sandboxd`) on a Linux host with KVM",
            )
        } else {
            (
                "the cluster control plane",
                "A key or session needs the `machines` scope: a key with only `inventory`, \
                 and an observer or viewer, are refused. A team's key sees its own team's \
                 machines only, and another team's machine is one that does not exist. The \
                 control plane asks every live node which holds a machine and relays to it.",
                &["machines"],
                "a control plane with at least one live node that runs machines",
            )
        };
        let suffix = if node {
            "node daemon"
        } else {
            "cluster control plane"
        };
        let describe = |what: &str| format!("{what} through {who}. {auth}");
        let lifecycle = [
            "list_machines",
            "create_machine",
            "get_machine",
            "resize_machine",
            "delete_machine",
            "control_machine",
            "get_machine_console",
            "send_machine_console_input",
            "get_machine_network_decisions",
        ];
        let commands = [
            "exec_in_machine",
            "list_machine_processes",
            "read_machine_file",
            "write_machine_file",
            "list_machine_runs",
            "start_machine_run",
            "get_machine_run",
            "write_machine_run_stdin",
            "signal_machine_run",
        ];
        let mut agents = vec![
            "list_machine_prompts",
            "start_machine_prompt",
            "get_machine_prompt",
            "interrupt_machine_prompt",
        ];
        if node {
            agents.extend([
                "list_machine_conversations",
                "create_machine_conversation",
                "get_machine_conversation",
                "delete_machine_conversation",
                "send_machine_conversation_message",
                "get_machine_conversation_transcript",
                "interrupt_machine_conversation",
            ]);
        }
        let ports = [
            "list_machine_ports",
            "publish_machine_port",
            "unpublish_machine_port",
            "open_machine_reverse_port",
        ];
        let snapshots = [
            "list_machine_snapshots",
            "take_machine_snapshot",
            "get_machine_snapshot",
            "delete_machine_snapshot",
            "restore_machine_snapshot",
            "download_machine_snapshot_disk",
            "list_machine_snapshot_files",
            "read_machine_snapshot_file",
            "archive_machine_snapshot_directory",
        ];
        let mut capabilities = vec![
            capability(
                "machine_lifecycle",
                &format!("Machine Lifecycle ({suffix})"),
                &describe(
                    "Create, list, inspect, resize, start, stop, restart and delete machines: \
                     long-lived VMs with their own ext4 root disk that survive a restart of \
                     their node, booted from a template or from a raw disk image by firmware; \
                     read and type at their serial console, and read what the egress gateway \
                     decided. A machine is named within its team: two teams may each have \
                     `web-01`. See docs/MACHINES.md",
                ),
                &lifecycle,
                node,
                &[running],
                perms,
            ),
            capability(
                "machine_commands",
                &format!("Machine Commands, Runs And Files ({suffix})"),
                &describe(
                    "Run a shell line in a running machine's guest as root and wait for it, \
                     start one and leave it (a run, whose newest 4 MiB of output the node \
                     keeps), write to its input, signal it, list the guest's processes, and \
                     read and write files in the guest. All of it goes through the guest \
                     agent, which a machine from a template always has and one from an image \
                     has when created with `guestAgent`. See docs/MACHINES.md",
                ),
                &commands,
                node,
                &["a running machine with a guest agent answering"],
                perms,
            ),
            capability(
                "machine_agents",
                &format!("Machine Coding Agents ({suffix})"),
                &describe(if node {
                    "Prompt a coding agent (Claude Code, Codex or a command of the caller's) \
                     that is installed in a machine, read what it printed as events, interrupt \
                     it, and hold a conversation with one over many turns. Prompts are runs \
                     and are kept as runs are; conversations are kept in the node's memory \
                     only. `unattended` lets the agent act without asking"
                } else {
                    "Prompt a coding agent (Claude Code, Codex or a command of the caller's) \
                     that is installed in a machine, read what it printed as events, and \
                     interrupt it. The control plane does not route conversations; they are \
                     the node's. `unattended` lets the agent act without asking"
                }),
                &agents,
                node,
                &["a running machine with a guest agent answering and a coding agent installed"],
                perms,
            ),
            capability(
                "machine_ports",
                &format!("Machine Ports ({suffix})"),
                &describe(
                    "Publish a port of a machine's guest on a port of its node (TCP, no \
                     login of its own), optionally also through the node's sandbox proxy, \
                     take it back, and receive a connection the guest makes out through an \
                     upgraded `hv2-reverse/1` request",
                ),
                &ports,
                node,
                &["a machine with a guest agent"],
                perms,
            ),
            capability(
                "machine_snapshots",
                &format!("Machine Disk Snapshots ({suffix})"),
                &describe(
                    "Snapshot a stopped machine's root disk (files, not memory); list, \
                     restore and delete the snapshots; and read one without booting it: \
                     download the raw disk, list a directory, read a file or download a \
                     directory as a gzipped tar from its ext filesystem. A snapshot holds \
                     every file on the disk, secrets included",
                ),
                &snapshots,
                node,
                &["a stopped machine, for taking and restoring a snapshot"],
                perms,
            ),
        ];
        if node {
            capabilities.push(capability(
                "machine_desktop",
                "Machine Desktop (node daemon)",
                &describe(
                    "Describe a machine's desktop (a virtual display with a window manager, \
                     served by noVNC), take a screenshot of it and act on it: move the \
                     pointer, click, drag, type and press keys. A desktop is a machine \
                     created from an image with `guestAgent` and `desktop`. The control \
                     plane does not route these",
                ),
                &[
                    "get_machine_desktop",
                    "get_machine_desktop_screenshot",
                    "act_on_machine_desktop",
                ],
                true,
                &["a running machine created with a `desktop`"],
                perms,
            ));
        }
        capabilities
    }

    /// The fork, snapshot and checkpoint capabilities of the control plane
    /// (`node` false) or the node.
    pub(super) fn build_checkpoint_capabilities(node: bool) -> Vec<Capability> {
        let (suffix, auth, fork_perms, checkpoint_perms): (&str, &str, &[&str], &[&str]) = if node {
            (
                "node daemon",
                "It checks only the cluster token when it has one.",
                &["cluster token"],
                &["cluster token"],
            )
        } else {
            (
                "cluster control plane",
                "A key or session needs the `sandboxes` scope (and `templates` to list \
                 snapshots); a team's key acts on its own team's sandboxes only.",
                &["sandboxes", "templates"],
                &["sandboxes"],
            )
        };
        vec![
            capability(
                "sandbox_fork_and_snapshots",
                &format!("Sandbox Fork And Snapshots ({suffix})"),
                &format!(
                    "Fork a running sandbox into up to 100 copies that resume from its \
                     memory, or snapshot it as a template new sandboxes start from, and list \
                     the snapshots. Both carry the source's memory and so everything in it. \
                     {auth} See docs/SANDBOXES.md and docs/TEAMS.md"
                ),
                &["fork_sandbox", "snapshot_sandbox", "list_snapshots"],
                node,
                &["a running sandbox started from a template"],
                fork_perms,
            ),
            capability(
                "sandbox_checkpoints",
                &format!("Sandbox Checkpoints ({suffix})"),
                &format!(
                    "Keep named checkpoints of a running sandbox (memory, processes and, with \
                     a disk, files), restore the sandbox to one in place, and read a \
                     checkpoint's files without touching the sandbox: list a directory, read \
                     a file or download a directory as a gzipped tar. {auth} See \
                     docs/CHECKPOINTS.md"
                ),
                &[
                    "create_sandbox_checkpoint",
                    "list_sandbox_checkpoints",
                    "delete_sandbox_checkpoint",
                    "restore_sandbox_checkpoint",
                    "list_sandbox_checkpoint_files",
                    "read_sandbox_checkpoint_file",
                    "archive_sandbox_checkpoint_directory",
                ],
                node,
                &["a running sandbox"],
                checkpoint_perms,
            ),
        ]
    }
}
