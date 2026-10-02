#!/usr/bin/env python3
"""Exercise readiness acknowledgements and transport reconnect semantics."""
import importlib.util
import json
from pathlib import Path
import struct
import time
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('fc_contract',Path(__file__).with_name('bench-firecracker-local.py'))
fc=importlib.util.module_from_spec(spec);spec.loader.exec_module(fc)


class Process:
    def poll(self):return None


class Stream:
    def __init__(self,kind,reset=False):
        response=json.dumps({'id':1,'version':3,'result':{'kind':kind}}).encode()
        self.data=bytearray(b'OK 3\n'+struct.pack('<I',len(response))+response)
        self.sent=[];self.closed=False;self.reset=reset
    def settimeout(self,value):pass
    def connect(self,path):pass
    def sendall(self,value):self.sent.append(value)
    def recv(self,size):
        if self.reset and len(self.sent)>1:raise ConnectionResetError('lost response after request')
        value=bytes(self.data[:size]);del self.data[:size];return value
    def close(self):self.closed=True


class ReadinessContract(unittest.TestCase):
    def connect(self,streams,readiness=None):
        with patch.object(fc.socket,'AF_UNIX',1,create=True), patch.object(fc.socket,'socket',side_effect=streams):
            return fc.guest('/owned/socket',time.perf_counter()+1,Process(),readiness=readiness)
    def test_existing_cold_readiness_still_sends_ping(self):
        stream=Stream('pong');self.assertIs(self.connect([stream]),stream)
        self.assertEqual(json.loads(stream.sent[1][4:])['op'],{'kind':'ping'})
    def test_restore_notice_replaces_ping_and_requires_acknowledgement(self):
        stream=Stream('acknowledged');operation={'kind':'restored','unix_time_ns':123,'entropy':[7]*64}
        self.assertIs(self.connect([stream],lambda:(operation,'acknowledged')),stream)
        self.assertEqual(json.loads(stream.sent[1][4:])['op'],operation)
        self.assertEqual(len(stream.sent),2)
    def test_pong_cannot_satisfy_a_restore_notice(self):
        stream=Stream('pong')
        with self.assertRaises(RuntimeError):self.connect([stream],lambda:({'kind':'restored'},'acknowledged'))
        self.assertTrue(stream.closed)
    def test_response_loss_reconnects_with_new_notice(self):
        first,second=Stream('acknowledged',reset=True),Stream('acknowledged');calls=[]
        def notice():
            token=len(calls)+1;calls.append(token)
            return ({'kind':'restored','unix_time_ns':token,'entropy':[token]*64},'acknowledged')
        self.assertIs(self.connect([first,second],notice),second)
        self.assertEqual(calls,[1,2]);self.assertTrue(first.closed)
        self.assertNotEqual(json.loads(first.sent[1][4:])['op'],json.loads(second.sent[1][4:])['op'])


if __name__=='__main__':unittest.main()
