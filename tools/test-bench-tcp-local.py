"""Verify transfer scoring rejects failed or corrupted responses."""
import importlib.util
from pathlib import Path
import socket
import threading
import unittest

spec = importlib.util.spec_from_file_location("bench", Path(__file__).with_name("bench-tcp-local.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class TransferTests(unittest.TestCase):
    def exercise(self, change):
        client, server = socket.socketpair()
        client.settimeout(2)
        server.settimeout(2)
        payload = bytes(range(256)) * 4096
        errors = []
        def serve():
            try:
                data = bytearray()
                while len(data) < len(payload):
                    chunk = server.recv(min(65536, len(payload) - len(data)))
                    if not chunk:
                        break
                    data.extend(chunk)
                server.sendall(change(bytes(data)))
            except Exception as error:
                errors.append(error)
            finally:
                server.close()
        worker = threading.Thread(target=serve)
        worker.start()
        result = bench.transfer(lambda: client, payload)
        worker.join(3)
        self.assertFalse(worker.is_alive())
        self.assertFalse(errors)
        self.assertEqual(client.fileno(), -1)
        return result

    def test_binary_transfer_larger_than_socket_buffers(self):
        row = self.exercise(lambda data: data)
        self.assertTrue(row["success"], row)
        self.assertGreater(row["transaction_ms"], 0)

    def test_corruption_is_not_scored(self):
        row = self.exercise(lambda data: b"x" + data[1:])
        self.assertFalse(row["success"])
        self.assertEqual(row["failure_phase"], "transfer")
        self.assertNotIn("aggregate_payload_mib_s", row)

    def test_premature_eof_is_not_scored(self):
        self.assertFalse(self.exercise(lambda data: data[:10])["success"])

    def test_handshake_failure_is_not_scored(self):
        def fail():
            raise ConnectionError("refused")
        row = bench.transfer(fail, b"test")
        self.assertFalse(row["success"])
        self.assertEqual(row["failure_phase"], "handshake")


if __name__ == "__main__":
    unittest.main()
