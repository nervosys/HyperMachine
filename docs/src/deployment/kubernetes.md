# Kubernetes Deployment

Deploy HyperMachine on Kubernetes using Helm.

## Prerequisites

- Kubernetes 1.25+
- Helm 3.0+
- Privileged container support (for hypervisor)

## Install with Helm

```bash
# Add Helm repository
helm repo add hypermachine https://charts.hypermachine.io
helm repo update

# Install
helm install hypermachine hypermachine/hypermachine \
  --namespace hypermachine \
  --create-namespace \
  --set apiKey="your-secret-key"
```

## Configuration

```yaml
# values.yaml
replicaCount: 3

image:
  repository: ghcr.io/nervosys/hypermachine
  tag: latest

resources:
  requests:
    cpu: "2"
    memory: "4Gi"
  limits:
    cpu: "8"
    memory: "16Gi"

service:
  type: LoadBalancer
  port: 8080

persistence:
  enabled: true
  storageClass: "fast-ssd"
  size: 100Gi

config:
  apiKey: ""  # Set via --set or secret
  logLevel: "info"
  
securityContext:
  privileged: true  # Required for KVM
```

## Production Setup

```bash
helm install hypermachine hypermachine/hypermachine \
  --namespace hypermachine \
  --create-namespace \
  --values production-values.yaml \
  --set apiKey="$(kubectl get secret hm-secrets -o jsonpath='{.data.api-key}' | base64 -d)"
```

## Scaling

```bash
# Scale replicas
kubectl scale deployment hypermachine --replicas=5

# Horizontal Pod Autoscaler
kubectl autoscale deployment hypermachine --min=3 --max=10 --cpu-percent=70
```
