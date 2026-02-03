/**
 * HyperMachine AI Agent Integration Example
 *
 * This example demonstrates how to use HyperMachine with OpenAI function
 * calling to let AI agents manage virtual machines autonomously.
 *
 * Requirements:
 *   npm install openai tsx @types/node
 *
 * Usage:
 *   OPENAI_API_KEY=your-api-key npx tsx ai_agent_integration.ts
 */

import OpenAI from "openai";
import type { ChatCompletionTool, ChatCompletionMessageParam } from "openai/resources/chat/completions";

// ============================================================================
// Type Definitions
// ============================================================================

interface HyperMachineToolsResponse {
  tools: ChatCompletionTool[];
}

interface ToolCallResult {
  [key: string]: unknown;
}

// ============================================================================
// Tool Execution
// ============================================================================

async function fetchHyperMachineTools(): Promise<ChatCompletionTool[]> {
  const response = await fetch("http://localhost:8080/agentic/tools/openai");
  if (!response.ok) {
    throw new Error(`Failed to fetch tools: ${response.status}`);
  }
  const data = (await response.json()) as HyperMachineToolsResponse;
  return data.tools;
}

async function executeToolCall(
  toolName: string,
  args: Record<string, unknown>
): Promise<string> {
  const baseUrl = "http://localhost:8080/api/v1";

  try {
    let response: Response;

    switch (toolName) {
      case "create_vm":
        response = await fetch(`${baseUrl}/vms`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(args),
        });
        break;

      case "list_vms":
        response = await fetch(`${baseUrl}/vms`);
        break;

      case "start_vm":
        response = await fetch(`${baseUrl}/vms/${args.vm_id}/start`, {
          method: "POST",
        });
        break;

      case "stop_vm":
        response = await fetch(`${baseUrl}/vms/${args.vm_id}/stop`, {
          method: "POST",
        });
        break;

      case "delete_vm":
        response = await fetch(`${baseUrl}/vms/${args.vm_id}`, {
          method: "DELETE",
        });
        return JSON.stringify({ status: "deleted" });

      case "get_vm_status":
        response = await fetch(`${baseUrl}/vms/${args.vm_id}`);
        break;

      case "execute_script":
        response = await fetch(`${baseUrl}/vms/${args.vm_id}/script`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            code: args.code,
            language: args.language ?? "rhai",
          }),
        });
        break;

      default:
        return JSON.stringify({ error: `Unknown tool: ${toolName}` });
    }

    if (!response.ok) {
      return JSON.stringify({
        error: `HTTP ${response.status}`,
        message: await response.text(),
      });
    }

    const result: ToolCallResult = await response.json();
    return JSON.stringify(result);
  } catch (error) {
    if (error instanceof TypeError && error.message.includes("fetch")) {
      return JSON.stringify({
        error: "Connection failed",
        message: "Could not connect to HyperMachine API",
      });
    }
    throw error;
  }
}

// ============================================================================
// AI Agent Runner
// ============================================================================

async function runAiAgent(userMessage: string): Promise<void> {
  const client = new OpenAI();

  // Fetch HyperMachine tools
  console.log("Fetching HyperMachine tools...");
  let tools: ChatCompletionTool[];

  try {
    tools = await fetchHyperMachineTools();
    console.log(`Loaded ${tools.length} tools`);
  } catch {
    console.log("Warning: Could not connect to HyperMachine API");
    console.log("Using mock tools for demonstration");
    tools = [
      {
        type: "function",
        function: {
          name: "create_vm",
          description: "Create a new virtual machine",
          parameters: {
            type: "object",
            properties: {
              name: { type: "string" },
              cpus: { type: "integer" },
              memory_mb: { type: "integer" },
            },
            required: ["name"],
          },
        },
      },
    ];
  }

  const messages: ChatCompletionMessageParam[] = [
    {
      role: "system",
      content:
        "You are a helpful assistant that manages virtual machines using " +
        "HyperMachine. When asked to perform VM operations, use the provided " +
        "tools to complete the task.",
    },
    { role: "user", content: userMessage },
  ];

  console.log(`\nUser: ${userMessage}`);
  console.log("-".repeat(40));

  // Run the conversation loop
  while (true) {
    const response = await client.chat.completions.create({
      model: "gpt-4",
      messages,
      tools,
      tool_choice: "auto",
    });

    const message = response.choices[0].message;

    // Check if we need to call tools
    if (message.tool_calls && message.tool_calls.length > 0) {
      messages.push(message);

      for (const toolCall of message.tool_calls) {
        console.log(`\nCalling tool: ${toolCall.function.name}`);
        console.log(`Arguments: ${toolCall.function.arguments}`);

        const result = await executeToolCall(
          toolCall.function.name,
          JSON.parse(toolCall.function.arguments) as Record<string, unknown>
        );

        console.log(`Result: ${result}`);

        messages.push({
          role: "tool",
          tool_call_id: toolCall.id,
          content: result,
        });
      }
    } else {
      // Final response
      console.log(`\nAssistant: ${message.content}`);
      break;
    }
  }
}

// ============================================================================
// Main
// ============================================================================

async function main(): Promise<void> {
  console.log("HyperMachine AI Agent Integration Example");
  console.log("=".repeat(50));

  // Check for API key
  if (!process.env.OPENAI_API_KEY) {
    console.log("\nNote: OPENAI_API_KEY not set");
    console.log("Set it to run the full example:");
    console.log("  export OPENAI_API_KEY='your-api-key'");
    return;
  }

  // Example tasks for the AI agent
  const tasks = [
    "Create a new VM named 'test-vm' with 2 CPUs and 2GB of RAM",
    "List all running virtual machines",
    "Start the VM named 'test-vm'",
  ];

  console.log("\nExample Tasks:");
  for (let i = 0; i < tasks.length; i++) {
    console.log(`  ${i + 1}. ${tasks[i]}`);
  }

  console.log("\n" + "=".repeat(50));
  console.log("Running first task...");
  await runAiAgent(tasks[0]);
}

main().catch(console.error);
