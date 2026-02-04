# REST API

HyperMachine provides a RESTful HTTP API for VM management.

## Base URL

```
http://localhost:8080/api/v1
```

## Authentication

All requests require an API key:

```bash
curl -H "Authorization: Bearer your-api-key" http://localhost:8080/api/v1/vms
```

## Endpoints

### Virtual Machines

#### List VMs

```http
GET /api/v1/vms
```

Query parameters:
- `status` - Filter by status (running, stopped, paused)
- `limit` - Max results (default: 100)
- `offset` - Pagination offset

Response:
```json
{
  "vms": [
    {
      "id": "vm-550e8400-e29b-41d4-a716-446655440000",
      "name": "my-vm",
      "status": "running",
      "cpu_cores": 4,
      "memory_mb": 8192,
      "created_at": "2025-01-15T10:30:00Z"
    }
  ],
  "total": 1
}
```

#### Create VM

```http
POST /api/v1/vms
Content-Type: application/json

{
  "name": "my-vm",
  "cpu_cores": 4,
  "memory_mb": 8192,
  "disk_gb": 100,
  "enable_gpu": true,
  "network_mode": "nat",
  "image": "ubuntu-22.04"
}
```

Response:
```json
{
  "id": "vm-550e8400-e29b-41d4-a716-446655440000",
  "name": "my-vm",
  "status": "created",
  "cpu_cores": 4,
  "memory_mb": 8192,
  "disk_gb": 100,
  "gpu_enabled": true,
  "network_mode": "nat",
  "created_at": "2025-01-15T10:30:00Z"
}
```

#### Get VM

```http
GET /api/v1/vms/{vm_id}
```

#### Delete VM

```http
DELETE /api/v1/vms/{vm_id}
```

#### Start VM

```http
POST /api/v1/vms/{vm_id}/start
```

#### Stop VM

```http
POST /api/v1/vms/{vm_id}/stop

{
  "force": false
}
```

#### Execute Command

```http
POST /api/v1/vms/{vm_id}/exec
Content-Type: application/json

{
  "command": "echo 'Hello, World!'",
  "timeout_secs": 60
}
```

Response:
```json
{
  "exit_code": 0,
  "stdout": "Hello, World!\n",
  "stderr": "",
  "duration_ms": 50
}
```

### Snapshots

#### Create Snapshot

```http
POST /api/v1/vms/{vm_id}/snapshots
Content-Type: application/json

{
  "name": "before-experiment"
}
```

#### List Snapshots

```http
GET /api/v1/vms/{vm_id}/snapshots
```

#### Restore Snapshot

```http
POST /api/v1/vms/{vm_id}/snapshots/{snapshot_id}/restore
```

### Files

#### Upload File

```http
POST /api/v1/vms/{vm_id}/files
Content-Type: application/json

{
  "path": "/home/user/data.txt",
  "content": "file contents here",
  "encoding": "utf-8"
}
```

#### Download File

```http
GET /api/v1/vms/{vm_id}/files?path=/home/user/data.txt
```

## Error Responses

```json
{
  "error": {
    "code": "VM_NOT_FOUND",
    "message": "Virtual machine not found",
    "details": {
      "vm_id": "vm-invalid-id"
    }
  }
}
```

## HTTP Status Codes

| Code | Description |
|------|-------------|
| 200 | Success |
| 201 | Created |
| 400 | Bad Request |
| 401 | Unauthorized |
| 404 | Not Found |
| 429 | Rate Limited |
| 500 | Internal Error |
