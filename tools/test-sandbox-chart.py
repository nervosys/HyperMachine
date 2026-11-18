#!/usr/bin/env python3
"""Render sandbox chart security configurations. Requires Helm and PyYAML."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import yaml

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--helm", default="helm")
options, remaining = parser.parse_known_args()
chart = Path(__file__).resolve().parents[1] / "deploy/helm/hypermachine-sandbox"


def render(*arguments):
    return subprocess.run([options.helm, "template", "fixture", str(chart), *arguments],
                          capture_output=True, text=True, timeout=30)


def deployment(result):
    assert result.returncode == 0, result.stderr
    resources = list(yaml.safe_load_all(result.stdout))
    return next(item["spec"]["template"]["spec"] for item in resources
                if item and item.get("kind") == "Deployment"
                and item["metadata"]["name"].endswith("-control-plane"))


def node(result):
    if result.returncode != 0:
        raise ValueError(result.stderr)
    resources = list(yaml.safe_load_all(result.stdout))
    return next(item["spec"]["template"]["spec"] for item in resources
                if item and item.get("kind") == "DaemonSet")


class ChartSecurityTests(unittest.TestCase):
    def test_guest_sizing_is_rendered_as_daemon_arguments(self):
        for cores, memory in ((1, 1), (2, 1), (4, 2)):
            with self.subTest(cores=cores, memory=memory):
                arguments = node(render("--set", f"node.cpuCores={cores},node.memoryGb={memory}"))["containers"][0]["args"]
                self.assertEqual(arguments[arguments.index("--cpu-cores") + 1], str(cores))
                self.assertEqual(arguments[arguments.index("--memory-gb") + 1], str(memory))

    def test_invalid_guest_sizing_is_rejected_before_rendering(self):
        for field, maximum in (("cpuCores", 4294967295), ("memoryGb", 18014398509481983)):
            for value in ("0", "-1", "true", "1.5", str(maximum + 1)):
                with self.subTest(field=field, value=value):
                    self.assertNotEqual(render("--set", f"node.{field}={value}").returncode, 0)
            self.assertNotEqual(render("--set-string", f"node.{field}=2").returncode, 0)

    def test_snapshot_and_volume_claims_have_independent_mounts_and_arguments(self):
        for snapshots, volumes in ((False, False), (True, False), (False, True), (True, True)):
            with self.subTest(snapshots=snapshots, volumes=volumes):
                settings = []
                if snapshots:
                    settings += ["--set", "node.snapshotStore.claimName=snapshots"]
                if volumes:
                    settings += ["--set", "node.volumeStore.claimName=volumes"]
                pod = node(render(*settings))
                container = pod["containers"][0]
                mounts = {m["name"]: m for m in container["volumeMounts"]}
                sources = {v["name"]: v for v in pod["volumes"]}
                for enabled, name, flag, path, claim in (
                    (snapshots, "snapshot-store", "--snapshot-store", "/var/lib/hv2-store", "snapshots"),
                    (volumes, "volume-store", "--volume-dir", "/var/lib/hv2-volumes", "volumes")):
                    self.assertEqual(flag in container["args"], enabled)
                    self.assertEqual(name in mounts, enabled)
                    self.assertEqual(name in sources, enabled)
                    if enabled:
                        self.assertEqual(container["args"][container["args"].index(flag) + 1], path)
                        self.assertEqual(mounts[name]["mountPath"], path)
                        self.assertFalse(mounts[name].get("readOnly", False))
                        self.assertEqual(sources[name]["persistentVolumeClaim"]["claimName"], claim)

    def test_storage_claim_names_require_strings(self):
        for setting in ("node.volumeStore.claimName=123", "node.snapshotStore.claimName=true"):
            self.assertNotEqual(render("--set", setting).returncode, 0)

    def test_default_node_does_not_mount_operator_files(self):
        pod = node(render())
        self.assertNotIn("initContainers", pod)
        self.assertNotIn("--egress-secrets-file", pod["containers"][0]["args"])
        self.assertNotIn("--egress-upstream-ca", pod["containers"][0]["args"])

    def test_operator_file_combinations_copy_only_selected_keys(self):
        for policy, ca in ((True, False), (False, True), (True, True)):
            with self.subTest(policy=policy, ca=ca):
                settings = []
                if policy:
                    settings += ["--set", "node.egressSecrets.secretName=policies,node.egressSecrets.key=custom.json"]
                if ca:
                    settings += ["--set", "node.egressUpstreamCa.secretName=roots,node.egressUpstreamCa.key=custom.pem"]
                pod = node(render(*settings))
                container = pod["containers"][0]
                init = pod["initContainers"][0]
                self.assertEqual(init["command"], ["/bin/sh", "-ec"])
                self.assertEqual(init["securityContext"]["runAsUser"], 0)
                self.assertEqual(container["securityContext"]["runAsUser"], 0)
                self.assertFalse(init["securityContext"]["allowPrivilegeEscalation"])
                self.assertTrue(init["securityContext"]["readOnlyRootFilesystem"])
                self.assertEqual(init["securityContext"]["capabilities"]["drop"], ["ALL"])
                mount = next(m for m in container["volumeMounts"] if m["name"] == "private-egress")
                self.assertTrue(mount["readOnly"])
                self.assertNotIn("egress-inputs", [m["name"] for m in container["volumeMounts"]])
                volumes = {v["name"]: v for v in pod["volumes"]}
                self.assertEqual(volumes["private-egress"]["emptyDir"]["medium"], "Memory")
                inputs = volumes["egress-inputs"]["projected"]
                self.assertEqual(inputs["defaultMode"], 0o400)
                self.assertEqual(len(inputs["sources"]), int(policy) + int(ca))
                for enabled, flag, name, key in ((policy, "--egress-secrets-file", "policy.json", "custom.json"),
                                                (ca, "--egress-upstream-ca", "roots.pem", "custom.pem")):
                    self.assertEqual(flag in container["args"], enabled)
                    if enabled:
                        self.assertEqual(container["args"][container["args"].index(flag) + 1], mount["mountPath"] + "/config/" + name)
                        self.assertTrue(any(source["secret"]["items"] == [{"key": key, "path": name}] for source in inputs["sources"]))

    @unittest.skipUnless(os.name == "posix", "Linux private-file fixture")
    def test_init_script_turns_projected_symlinks_into_private_regular_files(self):
        pod = node(render("--set", "node.egressSecrets.secretName=policies,node.egressUpstreamCa.secretName=roots"))
        script = pod["initContainers"][0]["args"][0]
        with tempfile.TemporaryDirectory(prefix="hm-chart-private-") as directory:
            root = Path(directory)
            inputs = root / "inputs"; inputs.mkdir()
            private = root / "private"; private.mkdir()
            data = inputs / "..data"; data.mkdir()
            for name in ("policy.json", "roots.pem"):
                (data / name).write_text("owned fixture " + name)
                (data / name).chmod(0o400)
                (inputs / name).symlink_to("..data/" + name)
            script = script.replace("/inputs/", str(inputs) + "/").replace("/private/", str(private) + "/")
            subprocess.run(["/bin/sh", "-ec", script], check=True, capture_output=True, timeout=10)
            parent = private / "config"
            self.assertEqual(parent.stat().st_mode & 0o777, 0o700)
            self.assertEqual(parent.stat().st_uid, os.geteuid())
            for name in ("policy.json", "roots.pem"):
                path = parent / name
                self.assertFalse(path.is_symlink())
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertEqual(path.stat().st_nlink, 1)
                self.assertEqual(path.stat().st_uid, os.geteuid())
                self.assertEqual(path.read_bytes(), (data / name).read_bytes())

    def test_invalid_operator_files_cannot_render(self):
        for setting in ("node.network=false,node.egressSecrets.secretName=policies",
                        "node.network=false,node.egressUpstreamCa.secretName=roots",
                        "node.egressSecrets.secretName=policies,node.egressSecrets.key=",
                        "node.egressUpstreamCa.secretName=roots,node.egressUpstreamCa.key=../roots.pem"):
            self.assertNotEqual(render("--set", setting).returncode, 0)
        self.assertNotEqual(render("--set-string", "node.network=false").returncode, 0)
    def test_default_keeps_admin_authentication(self):
        pod = deployment(render())
        container = pod["containers"][0]
        self.assertIn("HV2_API_KEY", [entry["name"] for entry in container["env"]])
        self.assertNotIn("--api-keys-file", container["args"])

    def test_policy_only_projects_the_file_and_preserves_cluster_auth(self):
        pod = deployment(render("--set", "auth.apiKeysSecret=policies,auth.apiKeysSecretKey=custom.json,auth.legacyAdminEnabled=false"))
        container = pod["containers"][0]
        environment = {entry["name"] for entry in container["env"]}
        self.assertNotIn("HV2_API_KEY", environment)
        self.assertTrue({"HV2_CLUSTER_TOKEN", "HV2_STORE_PASSWORD"} <= environment)
        path = container["args"][container["args"].index("--api-keys-file") + 1]
        mount = next(item for item in container["volumeMounts"] if item["name"] == "api-keys")
        volume = next(item for item in pod["volumes"] if item["name"] == "api-keys")
        self.assertTrue(mount["readOnly"])
        self.assertEqual(volume["secret"]["secretName"], "policies")
        self.assertEqual(volume["secret"]["items"][0]["key"], "custom.json")
        self.assertEqual(path, mount["mountPath"] + "/" + volume["secret"]["items"][0]["path"])

    def test_all_policy_and_tls_mount_combinations_render_coherently(self):
        for policy in (False, True):
            for mtls in (False, True):
                for store_tls in (False, True):
                    arguments = []
                    expected = set()
                    for enabled, value, name in [(policy, "auth.apiKeysSecret=policies", "api-keys"),
                                                  (mtls, "mtls.controlPlaneSecret=mtls", "mtls"),
                                                  (store_tls, "store.tlsSecret=store-tls", "store-tls")]:
                        if enabled:
                            arguments += ["--set", value]
                            expected.add(name)
                    pod = deployment(render(*arguments))
                    mounts = pod["containers"][0].get("volumeMounts") or []
                    self.assertEqual({item["name"] for item in mounts}, expected)
                    self.assertEqual({item["name"] for item in pod.get("volumes") or []}, expected)
                    self.assertTrue(all(item["readOnly"] for item in mounts))

    def test_store_has_one_acl_user_per_role_and_each_component_uses_its_own(self):
        # The chart's ACL (from files/redis-acl-table.txt) must equal what tools/redis-acl.py prints.
        for namespace in ("default", "team-a.prod_1"):
            result = render("--set", "store.namespace=" + namespace)
            self.assertEqual(result.returncode, 0, result.stderr)
            resources = [item for item in yaml.safe_load_all(result.stdout) if item]
            config = next(item for item in resources if item["kind"] == "ConfigMap"
                          and item["metadata"]["name"].endswith("-store-acl"))
            passwords = {role: role.lower() * 6 for role in ("CONTROL_PLANE", "NODE", "GATEWAY")}
            expected = subprocess.run(
                [sys.executable, str(chart.parents[2] / "tools/redis-acl.py"), "--namespace", namespace],
                capture_output=True, text=True, check=True,
                env={**os.environ, **{"HV2_ACL_%s_PASSWORD" % role: value for role, value in passwords.items()}}).stdout
            actual = config["data"]["users.acl"]
            for role, value in passwords.items():
                actual = actual.replace("#__HASH_%s__" % role, "#" + hashlib.sha256(value.encode()).hexdigest())
            self.assertEqual(actual.strip(), expected.strip())
        self.assertIn("user default off", actual)
        # The node's user keeps MULTI (redis-rs panics without it) and has no key records.
        node_line = next(line for line in actual.splitlines() if line.startswith("user hv2-node "))
        self.assertIn("+@transaction", node_line)
        self.assertNotIn("managed-key", node_line)
        urls = {}
        for name, spec in (("control-plane", deployment(result)), ("node", node(result))):
            container = spec["containers"][0]
            args = container["args"]
            urls[name] = args[args.index("--store" if name == "control-plane" else "--cluster-store") + 1]
            variable = next(item for item in container["env"] if item["name"] == "HV2_STORE_PASSWORD")
            self.assertEqual(variable["valueFrom"]["secretKeyRef"]["key"], "store-password-" + name)
        self.assertTrue(urls["control-plane"].startswith("redis://hv2-control-plane:$(HV2_STORE_PASSWORD)@"))
        self.assertTrue(urls["node"].startswith("redis://hv2-node:$(HV2_STORE_PASSWORD)@"))
        store = next(item for item in resources if item["kind"] == "Deployment"
                     and item["metadata"]["name"].endswith("-store"))["spec"]["template"]["spec"]
        self.assertNotIn("--requirepass", store["containers"][0]["args"])
        self.assertIn("--aclfile", store["containers"][0]["args"])

    def test_store_namespace_cannot_widen_the_acl(self):
        for namespace in ("a*", "a b", "a~b", "*"):
            self.assertNotEqual(render("--set", "store.namespace=" + namespace).returncode, 0)

    def test_store_passwords_are_validated_and_the_old_key_is_refused(self):
        self.assertNotEqual(render("--set", "auth.storePassword=" + "a" * 30).returncode, 0)
        self.assertNotEqual(render("--set", "auth.storePasswords.node=short").returncode, 0)
        self.assertNotEqual(render("--set", "auth.storePasswords.node=" + "a/" * 20).returncode, 0)
        self.assertEqual(render("--set", "auth.storePasswords.node=" + "a" * 30).returncode, 0)

    def test_external_store_uses_the_role_urls(self):
        result = render("--set", "store.deploy=false,store.url=redis://hv2-control-plane:x@h:6379,"
                                 "store.nodeUrl=redis://hv2-node:y@h:6379,networkPolicy.externalStoreCidrs={10.1.0.0/16}")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("redis://hv2-control-plane:x@h:6379", deployment(result)["containers"][0]["args"])
        self.assertIn("redis://hv2-node:y@h:6379", node(result)["containers"][0]["args"])

    def test_invalid_auth_configuration_cannot_render_an_open_api(self):
        for arguments in [("--set", "auth.legacyAdminEnabled=false"),
                          ("--set-string", "auth.legacyAdminEnabled=false"),
                          ("--set", "auth.apiKeysSecret=policies,auth.apiKeysSecretKey=")]:
            self.assertNotEqual(render(*arguments).returncode, 0)


if __name__ == "__main__":
    unittest.main(argv=[__file__, *remaining])
