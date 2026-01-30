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
    // Agentic AI Interface (for LLM integration)
    // ===========================================================================

    /**
     * Get the complete HyperMachine ontology for AI agent discovery.
     */
    async getOntology(): Promise<Record<string, unknown>> {
        return this.request<Record<string, unknown>>("GET", "/agentic/ontology");
    }

    /**
     * Get a quick summary of available operations.
     */
    async getCapabilities(): Promise<Record<string, unknown>> {
        return this.request<Record<string, unknown>>("GET", "/agentic/capabilities");
    }

    /**
     * Get the JSON Schema for validation.
     * @param compact - If true, return minimal schema for bandwidth-constrained scenarios
     */
    async getSchema(compact: boolean = false): Promise<Record<string, unknown>> {
        const path = compact ? "/agentic/schema/compact" : "/agentic/schema";
        return this.request<Record<string, unknown>>("GET", path);
    }

    /**
     * Get provider-specific configuration for an LLM.
     * @param provider - Provider name (openai, gpt-5, claude, claude-4.5, gemini, gemini-2.5, etc.)
     */
    async getProviderConfig(provider: string): Promise<{
        provider: string;
        tools: unknown;
        system_prompt: string;
        hints: {
            temperature: number;
            parallel_tool_calls: boolean;
            max_tokens?: number;
        };
    }> {
        return this.request("GET", `/agentic/providers/${encodeURIComponent(provider)}`);
    }

    /**
     * Get tools in OpenAI function calling format.
     * Supports: GPT-5, GPT-5-turbo, GPT-4o, o1, o3, etc.
     */
    async getOpenAITools(): Promise<OpenAITool[]> {
        return this.request<OpenAITool[]>("GET", "/agentic/tools/openai");
    }

    /**
     * Get tools in Anthropic tool use format.
     * Supports: Claude 4.5, Claude 4, Claude 3.5, Sonnet, Opus, Haiku
     */
    async getAnthropicTools(): Promise<Array<{
        name: string;
        description: string;
        input_schema: Record<string, unknown>;
    }>> {
        return this.request("GET", "/agentic/tools/anthropic");
    }

    /**
     * Get tools in Google Gemini format.
     * Supports: Gemini 2.5, Gemini 2.0, Gemini Flash
     */
    async getGeminiTools(): Promise<{
        function_declarations: Array<{
            name: string;
            description: string;
            parameters: Record<string, unknown>;
        }>;
    }> {
        return this.request("GET", "/agentic/tools/gemini");
    }

    // ===========================================================================
    // Tool Discovery (legacy MCP endpoint)
    // ===========================================================================

    /**
     * Get available tools in MCP format.
     * Note: Prefer getOpenAITools() or getAnthropicTools() for LLM integration.
     */
    async listTools(): Promise<ToolDefinition[]> {
        return this.request<ToolDefinition[]>("GET", "/mcp/tools");
    }

    /**
     * Get tools formatted for OpenAI's function calling API.
     * Note: Prefer getOpenAITools() for optimized LLM integration.
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
