#!/usr/bin/env python3
"""
HyperMachine Python Client

A simple Python client for interacting with the HyperMachine MCP HTTP server.
Compatible with OpenAI function calling and direct API usage.

Usage:
    # Direct API usage
    client = HyperMachineClient("http://localhost:8080", api_key="your-key")
    
    # List tools (for OpenAI function calling)
    tools = client.list_tools()
    
    # Create a VM
    vm = client.create_vm("my-vm", cpu_cores=4, memory_gb=8)
    
    # Start the VM
    client.start_vm("my-vm")
    
    # Get VM status
    status = client.get_vm("my-vm")
    print(status)
"""

import json
import requests
from typing import Any, Optional
from dataclasses import dataclass


@dataclass
class VmInfo:
    """Virtual machine information."""
    name: str
    cpu_cores: int
    memory_gb: int
    gpu_enabled: bool
    network_enabled: bool
    state: str
    created_at: str
    updated_at: str


class HyperMachineError(Exception):
    """Exception raised by HyperMachine client."""
    def __init__(self, message: str, status_code: int = None):
        self.message = message
        self.status_code = status_code
        super().__init__(self.message)


class HyperMachineClient:
    """Client for the HyperMachine MCP HTTP server."""
    
    def __init__(self, base_url: str = "http://localhost:8080", api_key: str = None):
        """
        Initialize the HyperMachine client.
        
        Args:
            base_url: The base URL of the MCP server (default: http://localhost:8080)
            api_key: Optional API key for authentication
        """
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.session = requests.Session()
        
        if api_key:
            self.session.headers["Authorization"] = f"Bearer {api_key}"
        self.session.headers["Content-Type"] = "application/json"
    
    def _request(self, method: str, path: str, data: dict = None) -> dict:
        """Make an HTTP request to the server."""
        url = f"{self.base_url}{path}"
        
        try:
            if method == "GET":
                response = self.session.get(url)
            elif method == "POST":
                response = self.session.post(url, json=data)
            elif method == "DELETE":
                response = self.session.delete(url)
            else:
                raise ValueError(f"Unsupported method: {method}")
            
            if response.status_code == 401:
                raise HyperMachineError("Unauthorized - check your API key", 401)
            elif response.status_code == 429:
                raise HyperMachineError("Rate limit exceeded", 429)
            elif response.status_code >= 400:
                error_msg = response.text or f"HTTP {response.status_code}"
                raise HyperMachineError(error_msg, response.status_code)
            
            return response.json()
        except requests.RequestException as e:
            raise HyperMachineError(f"Request failed: {e}")
    
    # =========================================================================
    # Agentic AI Interface (for LLM integration)
    # =========================================================================
    
    def get_ontology(self) -> dict:
        """
        Get the complete HyperMachine ontology for AI agent discovery.
        
        Returns:
            Full ontology with concepts, types, operations, and relationships
        """
        return self._request("GET", "/agentic/ontology")
    
    def get_capabilities(self) -> dict:
        """
        Get a quick summary of available operations.
        
        Returns:
            Capabilities summary with operation list
        """
        return self._request("GET", "/agentic/capabilities")
    
    def get_schema(self, compact: bool = False) -> dict:
        """
        Get the JSON Schema for validation.
        
        Args:
            compact: If True, return minimal schema for bandwidth-constrained scenarios
            
        Returns:
            JSON Schema document
        """
        path = "/agentic/schema/compact" if compact else "/agentic/schema"
        return self._request("GET", path)
    
    def get_provider_config(self, provider: str) -> dict:
        """
        Get provider-specific configuration for an LLM.
        
        Args:
            provider: Provider name (openai, gpt-5, claude, claude-4.5, gemini, gemini-2.5, etc.)
            
        Returns:
            Configuration with tools, system_prompt, and hints
        """
        return self._request("GET", f"/agentic/providers/{provider}")
    
    def get_openai_tools(self) -> list[dict]:
        """
        Get tools in OpenAI function calling format.
        
        Supports: GPT-5, GPT-5-turbo, GPT-4o, o1, o3, etc.
        
        Returns:
            List of tools in OpenAI format
        """
        return self._request("GET", "/agentic/tools/openai")
    
    def get_anthropic_tools(self) -> list[dict]:
        """
        Get tools in Anthropic tool use format.
        
        Supports: Claude 4.5, Claude 4, Claude 3.5, Sonnet, Opus, Haiku
        
        Returns:
            List of tools in Anthropic format
        """
        return self._request("GET", "/agentic/tools/anthropic")
    
    def get_gemini_tools(self) -> dict:
        """
        Get tools in Google Gemini format.
        
        Supports: Gemini 2.5, Gemini 2.0, Gemini Flash
        
        Returns:
            Tools in Gemini function declaration format
        """
        return self._request("GET", "/agentic/tools/gemini")
    
    # =========================================================================
    # Tool Discovery (legacy MCP endpoint)
    # =========================================================================
    
    def list_tools(self) -> list[dict]:
        """
        Get available tools in basic format.
        
        Note: Prefer get_openai_tools() or get_anthropic_tools() for LLM integration.
        
        Returns:
            List of tool definitions with name, description, and parameters
        """
        return self._request("GET", "/mcp/tools")
    
    def list_tools_openai_format(self) -> list[dict]:
        """
        Get tools formatted for OpenAI's function calling API.
        
        Note: Prefer get_openai_tools() for optimized LLM integration.
        
        Returns:
            List of tools in OpenAI's expected format
        """
        tools = self.list_tools()
        return [
            {
                "type": "function",
                "function": {
                    "name": tool["name"],
                    "description": tool["description"],
                    "parameters": tool["parameters"]
                }
            }
            for tool in tools
        ]
    
    def call_tool(self, tool_name: str, arguments: dict) -> dict:
        """
        Execute a tool call (MCP unified endpoint).
        
        Args:
            tool_name: The tool to call (e.g., "vm.create", "vm.start")
            arguments: Tool-specific arguments
            
        Returns:
            Tool execution result
        """
        return self._request("POST", "/mcp/call", {
            "tool": tool_name,
            "arguments": arguments
        })
    
    # =========================================================================
    # VM Management (REST API)
    # =========================================================================
    
    def list_vms(self) -> list[VmInfo]:
        """List all virtual machines."""
        vms = self._request("GET", "/vms")
        return [VmInfo(**vm) for vm in vms]
    
    def create_vm(
        self,
        name: str,
        cpu_cores: int = 2,
        memory_gb: int = 4,
        gpu_enabled: bool = False,
        network_enabled: bool = False
    ) -> VmInfo:
        """
        Create a new virtual machine.
        
        Args:
            name: Unique VM name
            cpu_cores: Number of virtual CPUs (default: 2)
            memory_gb: Memory size in GB (default: 4)
            gpu_enabled: Enable GPU passthrough (default: False)
            network_enabled: Enable networking (default: False)
            
        Returns:
            Created VM information
        """
        data = {
            "name": name,
            "cpu_cores": cpu_cores,
            "memory_gb": memory_gb,
            "gpu_enabled": gpu_enabled,
            "network_enabled": network_enabled
        }
        result = self._request("POST", "/vms", data)
        return VmInfo(**result)
    
    def get_vm(self, name: str) -> VmInfo:
        """Get details of a specific VM."""
        result = self._request("GET", f"/vms/{name}")
        return VmInfo(**result)
    
    def delete_vm(self, name: str) -> dict:
        """Delete a virtual machine."""
        return self._request("DELETE", f"/vms/{name}")
    
    def start_vm(self, name: str) -> dict:
        """Start a virtual machine."""
        return self._request("POST", f"/vms/{name}/start")
    
    def stop_vm(self, name: str) -> dict:
        """Stop a virtual machine."""
        return self._request("POST", f"/vms/{name}/stop")
    
    def get_metrics(self, name: str) -> dict:
        """Get VM metrics (CPU, memory usage, uptime)."""
        return self._request("GET", f"/vms/{name}/metrics")
    
    def execute_script(self, name: str, script: str, timeout: int = 300) -> dict:
        """
        Execute a script inside a running VM.
        
        Args:
            name: VM name
            script: Script content to execute
            timeout: Execution timeout in seconds (default: 300)
            
        Returns:
            Script execution result
        """
        return self._request("POST", f"/vms/{name}/script", {
            "script": script,
            "timeout_seconds": timeout
        })
    
    def health_check(self) -> dict:
        """Check server health status."""
        return self._request("GET", "/health")


# =============================================================================
# OpenAI Integration Example
# =============================================================================

def openai_agent_example():
    """
    Example of using HyperMachine with OpenAI function calling.
    
    Requires: pip install openai
    """
    try:
        import openai
    except ImportError:
        print("Install openai package: pip install openai")
        return
    
    # Initialize clients
    hm = HyperMachineClient(api_key="your-api-key")
    
    # Get tools in OpenAI format
    tools = hm.list_tools_openai_format()
    
    # Create chat completion with tools
    response = openai.chat.completions.create(
        model="gpt-4",
        messages=[
            {"role": "user", "content": "Create a VM named 'web-server' with 4 CPU cores and 8GB RAM, then start it"}
        ],
        tools=tools,
    )
    
    # Process tool calls from the response
    for tool_call in response.choices[0].message.tool_calls or []:
        result = hm.call_tool(
            tool_call.function.name,
            json.loads(tool_call.function.arguments)
        )
        print(f"Tool: {tool_call.function.name}")
        print(f"Result: {result}")


# =============================================================================
# Direct Usage Example
# =============================================================================

def direct_usage_example():
    """Example of direct API usage."""
    # Initialize client (no auth for local development)
    client = HyperMachineClient("http://localhost:8080")
    
    # Check health
    health = client.health_check()
    print(f"Server status: {health['status']}")
    
    # Create a VM
    vm = client.create_vm(
        name="example-vm",
        cpu_cores=2,
        memory_gb=4,
        network_enabled=True
    )
    print(f"Created VM: {vm.name} ({vm.state})")
    
    # Start the VM
    client.start_vm("example-vm")
    print("VM started")
    
    # Get metrics
    metrics = client.get_metrics("example-vm")
    print(f"CPU usage: {metrics.get('cpu_percent', 0)}%")
    print(f"Memory usage: {metrics.get('memory_percent', 0)}%")
    
    # List all VMs
    vms = client.list_vms()
    print(f"\nAll VMs ({len(vms)}):")
    for v in vms:
        print(f"  - {v.name}: {v.state} ({v.cpu_cores} cores, {v.memory_gb} GB)")
    
    # Stop and delete
    client.stop_vm("example-vm")
    client.delete_vm("example-vm")
    print("\nVM stopped and deleted")


if __name__ == "__main__":
    print("HyperMachine Python Client")
    print("=" * 40)
    print("\nTo use this client:")
    print("  1. Start the MCP server: hm serve --rest-port 8080")
    print("  2. Run: python hypermachine_client.py")
    print("\nExample usage:")
    print("  client = HyperMachineClient('http://localhost:8080')")
    print("  client.create_vm('my-vm', cpu_cores=4, memory_gb=8)")
    print("  client.start_vm('my-vm')")
