/**
 * HyperMachine TypeScript Client
 *
 * A TypeScript/JavaScript client for interacting with the HyperMachine MCP HTTP server.
 * Compatible with OpenAI function calling and direct API usage.
 *
 * @example
 * ```typescript
 * const client = new HyperMachineClient("http://localhost:8080", "your-api-key");
 *
 * // Create a VM
 * const vm = await client.createVm("my-vm", { cpuCores: 4, memoryGb: 8 });
 *
 * // Start it
 * await client.startVm("my-vm");
 *
 * // Get tools for OpenAI
 * const tools = await client.listToolsOpenAIFormat();
 * ```
 */

export interface VmInfo {
    name: string;
    cpu_cores: number;
    memory_gb: number;
    gpu_enabled: boolean;
    network_enabled: boolean;
    state: string;
    created_at: string;
    updated_at: string;
}

export interface ToolDefinition {
    name: string;
    description: string;
    parameters: Record<string, unknown>;
}

export interface OpenAITool {
    type: "function";
    function: {
        name: string;
        description: string;
        parameters: Record<string, unknown>;
    };
}

export interface ToolCallResult {
    success: boolean;
    result?: unknown;
    error?: string;
}

export interface VmCreateOptions {
    cpuCores?: number;
    memoryGb?: number;
    gpuEnabled?: boolean;
    networkEnabled?: boolean;
}

export interface VmMetrics {
    cpu_percent: number;
    memory_percent: number;
    uptime_seconds: number;
}

export class HyperMachineError extends Error {
    constructor(
        message: string,
        public statusCode?: number
    ) {
        super(message);
        this.name = "HyperMachineError";
    }
}

export class HyperMachineClient {
    private baseUrl: string;
    private apiKey?: string;

    /**
     * Create a new HyperMachine client.
     *
     * @param baseUrl - The base URL of the MCP server (default: http://localhost:8080)
     * @param apiKey - Optional API key for authentication
     */
    constructor(baseUrl: string = "http://localhost:8080", apiKey?: string) {
        this.baseUrl = baseUrl.replace(/\/$/, "");
        this.apiKey = apiKey;
    }

    private async request<T>(
        method: string,
        path: string,
        body?: unknown
    ): Promise<T> {
        const headers: Record<string, string> = {
            "Content-Type": "application/json",
        };

        if (this.apiKey) {
            headers["Authorization"] = `Bearer ${this.apiKey}`;
        }

        const response = await fetch(`${this.baseUrl}${path}`, {
            method,
            headers,
            body: body ? JSON.stringify(body) : undefined,
        });

        if (response.status === 401) {
            throw new HyperMachineError("Unauthorized - check your API key", 401);
        }
        if (response.status === 429) {
            throw new HyperMachineError("Rate limit exceeded", 429);
        }
        if (!response.ok) {
            const text = await response.text();
            throw new HyperMachineError(
                text || `HTTP ${response.status}`,
                response.status
            );
        }

        return response.json();
    }

    // ===========================================================================
    // Tool Discovery (for OpenAI/Anthropic integration)
    // ===========================================================================

    /**
     * Get available tools in MCP format.
     */
    async listTools(): Promise<ToolDefinition[]> {
        return this.request<ToolDefinition[]>("GET", "/mcp/tools");
    }

    /**
     * Get tools formatted for OpenAI's function calling API.
     */
    async listToolsOpenAIFormat(): Promise<OpenAITool[]> {
        const tools = await this.listTools();
        return tools.map((tool) => ({
            type: "function" as const,
            function: {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            },
        }));
    }

    /**
     * Execute a tool call via the MCP unified endpoint.
     */
    async callTool(toolName: string, args: unknown): Promise<ToolCallResult> {
        return this.request<ToolCallResult>("POST", "/mcp/call", {
            tool: toolName,
            arguments: args,
        });
    }

    // ===========================================================================
    // VM Management (REST API)
    // ===========================================================================

    /**
     * List all virtual machines.
     */
    async listVms(): Promise<VmInfo[]> {
        return this.request<VmInfo[]>("GET", "/vms");
    }

    /**
     * Create a new virtual machine.
     */
    async createVm(name: string, options: VmCreateOptions = {}): Promise<VmInfo> {
        return this.request<VmInfo>("POST", "/vms", {
            name,
            cpu_cores: options.cpuCores ?? 2,
            memory_gb: options.memoryGb ?? 4,
            gpu_enabled: options.gpuEnabled ?? false,
            network_enabled: options.networkEnabled ?? false,
        });
    }

    /**
     * Get details of a specific VM.
     */
    async getVm(name: string): Promise<VmInfo> {
        return this.request<VmInfo>("GET", `/vms/${encodeURIComponent(name)}`);
    }

    /**
     * Delete a virtual machine.
     */
    async deleteVm(name: string): Promise<{ status: string; name: string }> {
        return this.request("DELETE", `/vms/${encodeURIComponent(name)}`);
    }

    /**
     * Start a virtual machine.
     */
    async startVm(name: string): Promise<{ status: string; name: string }> {
        return this.request("POST", `/vms/${encodeURIComponent(name)}/start`);
    }

    /**
     * Stop a virtual machine.
     */
    async stopVm(name: string): Promise<{ status: string; name: string }> {
        return this.request("POST", `/vms/${encodeURIComponent(name)}/stop`);
    }

    /**
     * Get VM metrics.
     */
    async getMetrics(name: string): Promise<VmMetrics> {
        return this.request<VmMetrics>(
            "GET",
            `/vms/${encodeURIComponent(name)}/metrics`
        );
    }

    /**
     * Execute a script inside a running VM.
     */
    async executeScript(
        name: string,
        script: string,
        timeoutSeconds: number = 300
    ): Promise<unknown> {
        return this.request(
            "POST",
            `/vms/${encodeURIComponent(name)}/script`,
            {
                script,
                timeout_seconds: timeoutSeconds,
            }
        );
    }

    /**
     * Check server health.
     */
    async healthCheck(): Promise<{ status: string; service: string; version: string }> {
        return this.request("GET", "/health");
    }
}

// =============================================================================
// OpenAI Integration Example
// =============================================================================

/**
 * Example: Using HyperMachine with OpenAI function calling.
 *
 * ```typescript
 * import OpenAI from 'openai';
 * import { HyperMachineClient } from './hypermachine-client';
 *
 * const hm = new HyperMachineClient("http://localhost:8080", "your-api-key");
 * const openai = new OpenAI();
 *
 * // Get tools
 * const tools = await hm.listToolsOpenAIFormat();
 *
 * // Chat with tools
 * const response = await openai.chat.completions.create({
 *   model: "gpt-4",
 *   messages: [{ role: "user", content: "Create a VM with 4 cores" }],
 *   tools,
 * });
 *
 * // Execute tool calls
 * for (const toolCall of response.choices[0].message.tool_calls || []) {
 *   const result = await hm.callTool(
 *     toolCall.function.name,
 *     JSON.parse(toolCall.function.arguments)
 *   );
 *   console.log(result);
 * }
 * ```
 */

// =============================================================================
// Direct Usage Example
// =============================================================================

async function example() {
    const client = new HyperMachineClient("http://localhost:8080");

    // Check health
    const health = await client.healthCheck();
    console.log(`Server: ${health.status}`);

    // Create a VM
    const vm = await client.createVm("example-vm", {
        cpuCores: 2,
        memoryGb: 4,
        networkEnabled: true,
    });
    console.log(`Created: ${vm.name} (${vm.state})`);

    // Start it
    await client.startVm("example-vm");

    // List all VMs
    const vms = await client.listVms();
    console.log(`Total VMs: ${vms.length}`);

    // Cleanup
    await client.stopVm("example-vm");
    await client.deleteVm("example-vm");
}

// Run if executed directly
if (typeof require !== "undefined" && require.main === module) {
    example().catch(console.error);
}

export default HyperMachineClient;
