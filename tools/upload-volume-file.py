#!/usr/bin/env python3
"""Stream a file to a Linux sandbox node volume; atomic replacement is default."""
import argparse
import http.client
import json
import os
from pathlib import Path
import ssl
import socket
import stat
import threading
import time
import urllib.parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--endpoint', required=True, help='Node HTTPS URL or loopback HTTP URL')
    parser.add_argument('--ca-cert', type=Path)
    parser.add_argument('--volume-id', required=True)
    parser.add_argument('--path', required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--in-place', action='store_true')
    parser.add_argument('--no-clobber', action='store_true', help='Refuse replacing an existing destination')
    parser.add_argument('--force', action='store_true')
    parser.add_argument('--timeout', type=float, default=120)
    args = parser.parse_args()
    endpoint = urllib.parse.urlsplit(args.endpoint)
    if (endpoint.scheme not in ('http', 'https') or not endpoint.hostname
            or endpoint.username or endpoint.password or endpoint.query or endpoint.fragment
            or (endpoint.scheme == 'http' and endpoint.hostname not in ('localhost', '127.0.0.1', '::1'))):
        parser.error('endpoint requires HTTPS or loopback HTTP, without credentials/query/fragment')
    if not (1 <= len(args.volume_id) <= 64 and all(c.isascii() and (c.isalnum() or c in '_-') for c in args.volume_id)):
        parser.error('invalid volume ID')
    if not 0 < args.timeout <= 3600:
        parser.error('timeout must be positive and at most 3600 seconds')
    token = os.environ.get('HV2_VOLUME_TOKEN', '')
    if not token or any(ord(c) < 32 or ord(c) > 126 for c in token):
        parser.error('HV2_VOLUME_TOKEN must contain a nonempty valid bearer token')
    context = ssl.create_default_context(cafile=args.ca_cert) if endpoint.scheme == 'https' else None
    connection = (http.client.HTTPSConnection(endpoint.hostname, endpoint.port, context=context, timeout=args.timeout)
                  if context else http.client.HTTPConnection(endpoint.hostname, endpoint.port, timeout=args.timeout))
    query = urllib.parse.urlencode({'path': args.path, 'atomic': str(not args.in_place).lower(),
                                   'force': str(args.force).lower(), 'overwrite': str(not args.no_clobber).lower()})
    target = endpoint.path.rstrip('/') + '/volumecontent/' + args.volume_id + '/file?' + query
    watchdog = None
    try:
        # Nonblocking open lets fstat reject FIFOs without waiting for a writer.
        descriptor = os.open(args.source, os.O_RDONLY | getattr(os, 'O_NONBLOCK', 0) | getattr(os, 'O_BINARY', 0))
        with os.fdopen(descriptor, 'rb') as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 4 * 1024**3:
                raise ValueError('source must be a regular file of at most 4 GiB')
            deadline = time.monotonic() + args.timeout
            connection.connect()
            budget = deadline - time.monotonic()
            if budget <= 0:
                raise TimeoutError()
            connected_socket = connection.sock

            def expire():
                # Shutdown also interrupts reads through HTTPResponse's file
                # wrapper; closing the connection alone may leave that alive.
                try:
                    connected_socket.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

            watchdog = threading.Timer(budget, expire)
            watchdog.daemon = True
            watchdog.start()
            connection.putrequest('PUT', target)
            connection.putheader('Authorization', 'Bearer ' + token)
            connection.putheader('Content-Type', 'application/octet-stream')
            connection.putheader('Content-Length', str(metadata.st_size))
            connection.endheaders()
            remaining = metadata.st_size
            while remaining:
                chunk = source.read(min(128 * 1024, remaining))
                if not chunk:
                    raise ValueError('source ended during upload')
                budget = deadline - time.monotonic()
                if budget <= 0:
                    raise TimeoutError()
                connection.sock.settimeout(budget)
                connection.send(chunk)
                remaining -= len(chunk)
            budget = deadline - time.monotonic()
            if budget <= 0:
                raise TimeoutError()
            connection.sock.settimeout(budget)
            response = connection.getresponse()
            if response.status != 201:
                raise ValueError('node refused upload')
            body = response.read(65537)
            if len(body) > 65536:
                raise ValueError('upload response exceeds limit')
            result = json.loads(body)
            if result.get('size') != metadata.st_size:
                raise ValueError('uploaded size differs')
            if time.monotonic() >= deadline:
                raise TimeoutError()
            print(json.dumps(result))
    finally:
        if watchdog is not None:
            watchdog.cancel()
            watchdog.join(timeout=1)
        connection.close()


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('Volume upload failed; inspect the destination before retrying.')
