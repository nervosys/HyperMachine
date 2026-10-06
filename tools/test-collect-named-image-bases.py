#!/usr/bin/env python3
"""Test offline image GC preserves dependencies and rejects unsafe/stale plans."""
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('image_gc',Path(__file__).with_name('collect-named-image-bases.py'))
gc=importlib.util.module_from_spec(spec);spec.loader.exec_module(gc)

class CollectionTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='hm-image-gc-')
        self.root=Path(self.temp.name);(self.root/'.backup.lock').touch()
        (self.root/'snapshots').mkdir();(self.root/'paused').mkdir()
        self.image=self.root/'snapshots'/('source-'+'a'*32+'.snap.mem')
        self.image.write_bytes(b'x'*4096)
    def tearDown(self): self.temp.cleanup()
    def header(self,path,base=None,image=None):
        h={'vm_name':'fixture','memory_size':4096,'regions':[{'guest_addr':0,'size':4096,'readonly':False}],
           'vcpus':[],'total_pages':1,'present_pages':0,'device_state_included':True,
           'memory_base':base,'memory_image':image}
        raw=json.dumps(h).encode();path.write_bytes(b'HV2SNAP\0'+struct.pack('<II',2,len(raw))+raw+b'\0')
    def encoded_plan(self):
        plan=gc.plan(self.root,1024*1024);raw=json.dumps(plan).encode()
        return raw,hashlib.sha256(raw).hexdigest()
    def test_unreferenced_reclaimed(self):
        raw,sha=self.encoded_plan();result=gc.apply(self.root,raw,sha,1024*1024)
        self.assertTrue(result['success']);self.assertFalse(self.image.exists())
        self.assertTrue((self.root/'.backup.lock').exists())
    def test_paused_layer_retained(self):
        self.header(self.root/'paused/guest.snap',base=str(self.image))
        self.assertEqual(gc.plan(self.root,1024*1024)['delete'],[])
    def test_named_image_retained(self):
        self.header(self.root/'snapshots/named.snap',image=self.image.name)
        self.assertEqual(gc.plan(self.root,1024*1024)['delete'],[])
    def test_other_namespace_untouched(self):
        extra=self.root/'paused'/self.image.name;extra.write_bytes(b'y')
        raw,sha=self.encoded_plan();gc.apply(self.root,raw,sha,1024*1024)
        self.assertTrue(extra.exists())
    def test_live_node_lock_refused(self):
        with (self.root/'.backup.lock').open('rb') as lock:
            fcntl.flock(lock,fcntl.LOCK_SH|fcntl.LOCK_NB)
            with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)
        self.assertTrue(self.image.exists())
    def test_stale_plan_refused_before_unlink(self):
        raw,sha=self.encoded_plan();(self.root/'new-data').write_bytes(b'changed')
        with self.assertRaises(ValueError): gc.apply(self.root,raw,sha,1024*1024)
        self.assertTrue(self.image.exists())
    def test_changed_image_refused(self):
        raw,sha=self.encoded_plan();self.image.write_bytes(b'z'*4096)
        with self.assertRaises(ValueError): gc.apply(self.root,raw,sha,1024*1024)
        self.assertTrue(self.image.exists())
    def test_wrong_checksum_refused(self):
        raw,_=self.encoded_plan()
        with self.assertRaises(ValueError): gc.apply(self.root,raw,'0'*64,1024*1024)
        self.assertTrue(self.image.exists())
    def test_symlink_refused(self):
        (self.root/'alias').symlink_to(self.image)
        with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)
    def test_hardlink_orphan_refused(self):
        import os
        os.link(self.image,self.root/'alias')
        with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)
    def test_missing_dependency_refused(self):
        self.header(self.root/'paused/guest.snap',base=str(self.root/'missing.mem'))
        with self.assertRaises((ValueError,FileNotFoundError)): gc.plan(self.root,1024*1024)
    def test_outside_dependency_refused(self):
        with tempfile.TemporaryDirectory() as outside:
            image=Path(outside)/'outside.mem';image.write_bytes(b'x'*4096)
            self.header(self.root/'paused/guest.snap',base=str(image))
            with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)
    def test_corrupt_header_refused(self):
        (self.root/'paused/guest.snap').write_bytes(b'broken')
        with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)
    def test_snapshot_length_refused(self):
        path=self.root/'paused/guest.snap';self.header(path,base=str(self.image))
        with path.open('ab') as stream: stream.write(b'extra')
        with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)
    def test_ambiguous_named_record_refused(self):
        (self.root/'snapshots/named.json').write_text('{"file":"missing.snap"}')
        with self.assertRaises(ValueError): gc.plan(self.root,1024*1024)

if __name__=='__main__': unittest.main()
