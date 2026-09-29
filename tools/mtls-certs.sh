#!/usr/bin/env bash
# Certificates for mutual TLS between hv2-control-plane and hv2-sandboxd.
#
#   tools/mtls-certs.sh OUT_DIR [DAYS]
#
# Writes, into OUT_DIR:
#   ca.pem, ca.key                  the cluster's CA -- keep ca.key offline
#   node.pem, node.key              every node's certificate: DNS name
#                                   hv2-node, server and client auth
#   control-plane.pem, .key         every control plane's: client auth
#
# Then:
#   hv2-sandboxd ... --mtls-ca ca.pem --mtls-cert node.pem --mtls-key node.key \
#       --advertise-api https://ADDR:3980
#   hv2-control-plane ... --mtls-ca ca.pem --mtls-cert control-plane.pem \
#       --mtls-key control-plane.key
#
# One certificate for all nodes, because nodes are verified by that name
# rather than by address (see hv2_cluster::mtls). Rotate by issuing new ones
# from the same CA and restarting; to revoke, replace the CA. A cluster with
# cert-manager can issue the same shapes instead: the chart takes a Secret.
set -euo pipefail

out=${1:?usage: mtls-certs.sh OUT_DIR [DAYS]}
days=${2:-365}
mkdir -p "$out"
cd "$out"
umask 077

key() { openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1" 2>/dev/null; }

key ca.key
openssl req -x509 -new -key ca.key -days "$days" -subj "/CN=HyperMachine sandbox cluster CA" \
    -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" -out ca.pem

issue() {
    local name=$1 usage=$2
    key "$name.key"
    openssl req -new -key "$name.key" -subj "/CN=$3" -out "$name.csr"
    printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=%s\nsubjectAltName=DNS:%s\n' \
        "$usage" "$3" > "$name.ext"
    openssl x509 -req -in "$name.csr" -CA ca.pem -CAkey ca.key -CAcreateserial \
        -days "$days" -extfile "$name.ext" -out "$name.pem" 2>/dev/null
    rm -f "$name.csr" "$name.ext"
}
issue node serverAuth,clientAuth hv2-node
issue control-plane clientAuth hv2-control-plane
rm -f ca.srl
chmod 644 ./*.pem
echo "mtls-certs.sh: wrote $(ls | tr '\n' ' ')to $out"
