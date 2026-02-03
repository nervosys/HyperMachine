#!/usr/bin/env python3
"""
HyperMachine Python SDK Example

This example demonstrates how to use the HyperMachine REST API from Python
to create, manage, and interact with virtual machines.

Requirements:
    pip install httpx asyncio

Usage:
    python basic_usage.py
"""

import asyncio
import httpx
from dataclasses import dataclass
from typing import Optional, List

# HyperMachine API client
BASE_URL = "http://localhost:8080/api/v1"


@dataclass
class VmConfig:
    """Virtual machine configuration."""
    name: str
    cpus: int = 2
    memory_mb: int = 2048
    disk_gb: int = 20
    image: Optional[str] = None


@dataclass
class Vm:
    """Virtual machine instance."""
    id: str
    name: str
    state: str
    cpus: int
    memory_mb: int


class HyperMachineClient:
    """HyperMachine API client."""

    def __init__(self, base_url: str = BASE_URL, api_key: Optional[str] = None):
        self.base_url = base_url
        headers = {}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        self.client = httpx.AsyncClient(base_url=base_url, headers=headers)

    async def health(self) -> dict:
        """Check API health."""
        response = await self.client.get("/health")
        response.raise_for_status()
        return response.json()

    async def create_vm(self, config: VmConfig) -> Vm:
        """Create a new virtual machine."""
        response = await self.client.post(
            "/vms",
            json={
                "name": config.name,
                "cpus": config.cpus,
                "memory_mb": config.memory_mb,
                "disk_gb": config.disk_gb,
                "image": config.image,
            }
        )
        response.raise_for_status()
        data = response.json()
        return Vm(
            id=data["id"],
            name=data["name"],
            state=data["state"],
            cpus=data.get("cpus", config.cpus),
            memory_mb=data.get("memory_mb", config.memory_mb),
        )

    async def list_vms(self) -> List[Vm]:
        """List all virtual machines."""
        response = await self.client.get("/vms")
        response.raise_for_status()
        return [
            Vm(
                id=vm["id"],
                name=vm["name"],
                state=vm["state"],
                cpus=vm.get("cpus", 0),
                memory_mb=vm.get("memory_mb", 0),
            )
            for vm in response.json()["vms"]
        ]

    async def get_vm(self, vm_id: str) -> Vm:
        """Get VM details."""
        response = await self.client.get(f"/vms/{vm_id}")
        response.raise_for_status()
        data = response.json()
        return Vm(
            id=data["id"],
            name=data["name"],
            state=data["state"],
            cpus=data.get("cpus", 0),
            memory_mb=data.get("memory_mb", 0),
        )

    async def start_vm(self, vm_id: str) -> dict:
        """Start a virtual machine."""
        response = await self.client.post(f"/vms/{vm_id}/start")
        response.raise_for_status()
        return response.json()

    async def stop_vm(self, vm_id: str) -> dict:
        """Stop a virtual machine."""
        response = await self.client.post(f"/vms/{vm_id}/stop")
        response.raise_for_status()
        return response.json()

    async def delete_vm(self, vm_id: str) -> None:
        """Delete a virtual machine."""
        response = await self.client.delete(f"/vms/{vm_id}")
        response.raise_for_status()

    async def execute_script(
        self,
        vm_id: str,
        code: str,
        language: str = "rhai"
    ) -> dict:
        """Execute a script in the VM."""
        response = await self.client.post(
            f"/vms/{vm_id}/script",
            json={"code": code, "language": language}
        )
        response.raise_for_status()
        return response.json()

    async def close(self):
        """Close the client."""
        await self.client.aclose()


async def main():
    """Main example function."""
    print("HyperMachine Python SDK Example")
    print("=" * 40)

    # Create client
    client = HyperMachineClient()

    try:
        # Check health
        print("\n1. Checking API health...")
        health = await client.health()
        print(f"   Status: {health['status']}")
        print(f"   Version: {health['version']}")

        # Create VM
        print("\n2. Creating VM...")
        config = VmConfig(
            name="python-example-vm",
            cpus=2,
            memory_mb=2048,
            disk_gb=10
        )
        vm = await client.create_vm(config)
        print(f"   Created: {vm.name} (ID: {vm.id})")
        print(f"   State: {vm.state}")

        # List VMs
        print("\n3. Listing VMs...")
        vms = await client.list_vms()
        for v in vms:
            print(f"   - {v.name}: {v.state}")

        # Start VM
        print("\n4. Starting VM...")
        result = await client.start_vm(vm.id)
        print(f"   Result: {result.get('status', 'started')}")

        # Get VM details
        print("\n5. Getting VM details...")
        vm = await client.get_vm(vm.id)
        print(f"   State: {vm.state}")
        print(f"   CPUs: {vm.cpus}")
        print(f"   Memory: {vm.memory_mb} MB")

        # Execute script
        print("\n6. Executing script...")
        script_result = await client.execute_script(
            vm.id,
            'print("Hello from HyperMachine!");'
        )
        print(f"   Output: {script_result.get('output', 'N/A')}")

        # Stop VM
        print("\n7. Stopping VM...")
        await client.stop_vm(vm.id)
        print("   VM stopped")

        # Delete VM
        print("\n8. Deleting VM...")
        await client.delete_vm(vm.id)
        print("   VM deleted")

        print("\n" + "=" * 40)
        print("Example completed successfully!")

    except httpx.HTTPStatusError as e:
        print(f"\nAPI Error: {e.response.status_code}")
        print(f"Response: {e.response.text}")
    except httpx.ConnectError:
        print("\nError: Could not connect to HyperMachine API")
        print("Make sure the server is running: hm serve --port 8080")
    finally:
        await client.close()


if __name__ == "__main__":
    asyncio.run(main())
