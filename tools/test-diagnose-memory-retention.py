#!/usr/bin/env python3
"""Check procfs mapping boundaries and memory fields used by retention diagnostics."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("retention", Path(__file__).with_name("diagnose-memory-retention.py"))
retention = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retention)


class MappingParser(unittest.TestCase):
    def test_anonymous_and_file_mappings_keep_separate_fields_and_space_paths(self):
        records = retention.mappings("""1000-2000 rw-p 00000000 00:00 0
Size:                  4 kB
Rss:                   4 kB
Pss:                   3 kB
Private_Dirty:         4 kB
Anonymous:             4 kB
AnonHugePages:         0 kB
VmFlags: rd wr mr mw me ac sd
3000-4000 r-xp 00000000 08:01 12 /tmp/path with spaces (deleted)
Size:                  4 kB
Rss:                   2 kB
Pss:                   1 kB
""")
        self.assertEqual(len(records), 2)
        self.assertEqual(records[0]["path"], "")
        self.assertEqual(records[0]["permissions"], "rw-p")
        self.assertEqual(records[0]["Anonymous_kib"], 4)
        self.assertEqual(records[1]["path"], "/tmp/path with spaces (deleted)")
        self.assertEqual([record["Pss_kib"] for record in records], [3, 1])
        self.assertNotIn("Private_Dirty_kib", records[1])

    def test_large_virtual_mapping_keeps_virtual_size_distinct_from_resident_pss(self):
        records = retention.mappings("1000-40001000 rw-p 00000000 00:00 0\nSize: 1048576 kB\nRss: 8192 kB\nPss: 8192 kB\n")
        self.assertEqual(records[0]["Size_kib"], 1048576)
        self.assertEqual(records[0]["Pss_kib"], 8192)

    def test_invalid_numeric_memory_fails_instead_of_fabricating_a_zero(self):
        with self.assertRaises(ValueError):
            retention.mappings("1000-2000 rw-p 00000000 00:00 0\nPss: unavailable\n")


if __name__ == "__main__":
    unittest.main()
