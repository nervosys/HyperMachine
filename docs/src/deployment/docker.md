# Docker Deployment

Run HyperMachine in Docker containers.

## Quick Start

```bash
docker run -it --privileged \
  -p 8080:8080 \
  -v /var/lib/hypermachine:/data \
  -e HM_API_KEY="your-secret-key" \
  ghcr.io/nervosys/hypermachine:latest
```

## Docker Compose

```yaml
# docker-compose.yml
version: '3.8'

services:
  hypermachine:
    image: ghcr.io/nervosys/hypermachine:latest
    privileged: true
    ports:
      - "8080:8080"
      - "50051:50051"
    volumes:
      - hm-data:/data
      - /dev/kvm:/dev/kvm
    environment:
      - HM_API_KEY=${HM_API_KEY}
      - HM_LOG_LEVEL=info
    restart: unless-stopped

volumes:
  hm-data:
```

## Run

```bash
# Start
docker-compose up -d

# View logs
docker-compose logs -f

# Stop
docker-compose down
```

## GPU Support

```yaml
services:
  hypermachine:
    image: ghcr.io/nervosys/hypermachine:latest
    privileged: true
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: all
              capabilities: [gpu]
```

## Building Custom Image

```dockerfile
FROM ghcr.io/nervosys/hypermachine:latest

COPY config.toml /etc/hypermachine/config.toml
COPY custom-scripts/ /opt/scripts/

ENV HM_CONFIG_FILE=/etc/hypermachine/config.toml
```
