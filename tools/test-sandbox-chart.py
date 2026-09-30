#!/usr/bin/env python3
"""Render sandbox chart security configurations. Requires Helm and PyYAML."""
import argparse
from pathlib import Path
import subprocess
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


class ChartSecurityTests(unittest.TestCase):
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

    def test_invalid_auth_configuration_cannot_render_an_open_api(self):
        for arguments in [("--set", "auth.legacyAdminEnabled=false"),
                          ("--set-string", "auth.legacyAdminEnabled=false"),
                          ("--set", "auth.apiKeysSecret=policies,auth.apiKeysSecretKey=")]:
            self.assertNotEqual(render(*arguments).returncode, 0)


if __name__ == "__main__":
    unittest.main(argv=[__file__, *remaining])
