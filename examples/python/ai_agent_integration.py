#!/usr/bin/env python3
"""
HyperMachine AI Agent Integration Example

This example shows how to use HyperMachine with OpenAI function calling
to let AI agents manage virtual machines autonomously.

Requirements:
    pip install openai httpx

Usage:
    export OPENAI_API_KEY="your-api-key"
    python ai_agent_integration.py
"""

import os
import json
import httpx
from openai import OpenAI


def get_hypermachine_tools():
    """Fetch OpenAI-compatible tool definitions from HyperMachine."""
    response = httpx.get("http://localhost:8080/agentic/tools/openai")
    response.raise_for_status()
    return response.json()["tools"]


def execute_tool_call(tool_name: str, arguments: dict) -> str:
    """Execute a HyperMachine tool call."""
    base_url = "http://localhost:8080/api/v1"

    try:
        if tool_name == "create_vm":
            response = httpx.post(
                f"{base_url}/vms",
                json=arguments,
                timeout=30.0
            )
        elif tool_name == "list_vms":
            response = httpx.get(f"{base_url}/vms", timeout=10.0)
        elif tool_name == "start_vm":
            response = httpx.post(
                f"{base_url}/vms/{arguments['vm_id']}/start",
                timeout=30.0
            )
        elif tool_name == "stop_vm":
            response = httpx.post(
                f"{base_url}/vms/{arguments['vm_id']}/stop",
                timeout=30.0
            )
        elif tool_name == "delete_vm":
            response = httpx.delete(
                f"{base_url}/vms/{arguments['vm_id']}",
                timeout=30.0
            )
            return json.dumps({"status": "deleted"})
        elif tool_name == "get_vm_status":
            response = httpx.get(
                f"{base_url}/vms/{arguments['vm_id']}",
                timeout=10.0
            )
        elif tool_name == "execute_script":
            response = httpx.post(
                f"{base_url}/vms/{arguments['vm_id']}/script",
                json={
                    "code": arguments["code"],
                    "language": arguments.get("language", "rhai")
                },
                timeout=60.0
            )
        else:
            return json.dumps({"error": f"Unknown tool: {tool_name}"})

        response.raise_for_status()
        return json.dumps(response.json())

    except httpx.HTTPStatusError as e:
        return json.dumps({
            "error": f"HTTP {e.response.status_code}",
            "message": e.response.text
        })
    except httpx.ConnectError:
        return json.dumps({
            "error": "Connection failed",
            "message": "Could not connect to HyperMachine API"
        })


def run_ai_agent(user_message: str):
    """Run an AI agent with HyperMachine tools."""
    client = OpenAI()

    # Get HyperMachine tools
    print("Fetching HyperMachine tools...")
    try:
        tools = get_hypermachine_tools()
        print(f"Loaded {len(tools)} tools")
    except httpx.ConnectError:
        print("Warning: Could not connect to HyperMachine API")
        print("Using mock tools for demonstration")
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "create_vm",
                    "description": "Create a new virtual machine",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "cpus": {"type": "integer"},
                            "memory_mb": {"type": "integer"}
                        },
                        "required": ["name"]
                    }
                }
            }
        ]

    messages = [
        {
            "role": "system",
            "content": (
                "You are a helpful assistant that manages virtual machines "
                "using HyperMachine. When asked to perform VM operations, "
                "use the provided tools to complete the task."
            )
        },
        {"role": "user", "content": user_message}
    ]

    print(f"\nUser: {user_message}")
    print("-" * 40)

    # Run the conversation loop
    while True:
        response = client.chat.completions.create(
            model="gpt-4",
            messages=messages,
            tools=tools,
            tool_choice="auto"
        )

        message = response.choices[0].message

        # Check if we need to call tools
        if message.tool_calls:
            messages.append(message)

            for tool_call in message.tool_calls:
                print(f"\nCalling tool: {tool_call.function.name}")
                print(f"Arguments: {tool_call.function.arguments}")

                result = execute_tool_call(
                    tool_call.function.name,
                    json.loads(tool_call.function.arguments)
                )

                print(f"Result: {result}")

                messages.append({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": result
                })
        else:
            # Final response
            print(f"\nAssistant: {message.content}")
            break


def main():
    """Main function demonstrating AI agent integration."""
    print("HyperMachine AI Agent Integration Example")
    print("=" * 50)

    # Check for API key
    if not os.getenv("OPENAI_API_KEY"):
        print("\nNote: OPENAI_API_KEY not set")
        print("Set it to run the full example:")
        print("  export OPENAI_API_KEY='your-api-key'")
        return

    # Example tasks for the AI agent
    tasks = [
        "Create a new VM named 'test-vm' with 2 CPUs and 2GB of RAM",
        "List all running virtual machines",
        "Start the VM named 'test-vm'",
    ]

    print("\nExample Tasks:")
    for i, task in enumerate(tasks, 1):
        print(f"  {i}. {task}")

    print("\n" + "=" * 50)
    print("Running first task...")
    run_ai_agent(tasks[0])


if __name__ == "__main__":
    main()
