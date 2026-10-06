#!/usr/bin/env python3
"""Official-client verification of a configured read-only MCP API role."""
import argparse
import asyncio
import base64
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import ssl

import httpx
from mcp import ClientSession
from mcp.client.streamable_http import streamablehttp_client


async def check(args):
    context=ssl.create_default_context(cafile=str(args.ca))
    def factory(headers=None,timeout=None,auth=None):
        return httpx.AsyncClient(headers=headers,timeout=timeout,auth=auth,verify=context,follow_redirects=False)
    events=[]
    async with streamablehttp_client(args.url,headers={'Authorization':'Bearer '+os.environ['HM_MCP_TOKEN']},httpx_client_factory=factory) as (read,write,_):
        async with ClientSession(read,write) as session:
            await session.initialize()
            tools=await session.list_tools()
            if len(tools.tools)!=(14 if args.files else 12):raise ValueError('tool discovery differs')
            async def call(name,arguments=None,denied=False):
                result=await session.call_tool(name,arguments or {})
                if bool(result.isError)!=denied:raise ValueError('unexpected role result: '+name)
                raw=result.model_dump_json()
                if 'envdAccessToken' in raw or 'accessToken' in raw:raise ValueError('credential-bearing descriptor leaked')
                events.append({'tool':name,'denied':denied})
                if not denied:return json.loads(result.content[0].text)
            inventory=await call('sandbox_list')
            if not any(row.get('sandboxID')==args.target for row in inventory):raise ValueError('observer cannot see owned fixture inventory')
            for name,arguments in [
                ('sandbox_create',{}),('sandbox_inspect',{'id':args.target}),
                ('sandbox_exec',{'id':args.target,'command':['/bin/sh','-c','printf changed > /tmp/observer-marker']}),
                ('sandbox_pause',{'id':args.target}),('sandbox_resume',{'id':args.target}),
                ('sandbox_fork',{'id':args.target}),('sandbox_delete',{'id':args.target}),
                ('checkpoint_save',{'id':args.target,'name':'observer'}),('checkpoint_list',{'id':args.target}),
                ('checkpoint_restore',{'id':args.target,'name':'observer'}),('checkpoint_delete',{'id':args.target,'name':'observer'}),
            ]:await call(name,arguments,True)
            if args.files:
                await call('file_upload',{'id':args.target,'path':'/tmp/observer-marker','data_base64':base64.b64encode(b'changed').decode()},True)
                await call('file_download',{'id':args.target,'path':'/tmp/observer-marker'},True)
            after=await call('sandbox_list')
            if {row['sandboxID'] for row in inventory}!={row['sandboxID'] for row in after}:raise ValueError('observer altered inventory')
            await session.send_ping()
    return {'success':True,'client':'official-mcp-python','client_version':importlib.metadata.version('mcp'),
            'harness_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'operations':events,
            'denied_operations':sum(row['denied'] for row in events),'inventory_unchanged':True,'tokens_scrubbed':True,
            'scope':'one MCP process inherits its configured observer API role; no multi-user endpoint isolation claim'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url',required=True);parser.add_argument('--ca',type=Path,required=True);parser.add_argument('--target',required=True);parser.add_argument('--files',action='store_true')
    print(json.dumps(asyncio.run(check(parser.parse_args())),indent=2))
