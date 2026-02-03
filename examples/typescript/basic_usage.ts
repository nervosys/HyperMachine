/**
 * HyperMachine TypeScript SDK Example
 *
 * This example demonstrates how to interact with the HyperMachine API
 * using TypeScript with full type safety.
 *
 * Requirements:
 *   npm install typescript tsx @types/node
 *
 * Usage:
 *   npx tsx basic_usage.ts
 */

// ============================================================================
// Type Definitions
// ============================================================================

interface VmConfig {
  name: string;
  cpus?: number;
  memoryMb?: number;
  diskPath?: string;
  networkEnabled?: boolean;
  metadata?: Record<string, string>;
}

interface Vm {
  id: string;
  name: string;
  state: "stopped" | "running" | "paused" | "error";
  cpus: number;
  memoryMb: number;
  createdAt: string;
  startedAt?: string;
}

interface HealthResponse {
  status: "healthy" | "degraded" | "unhealthy";
  version: string;
  uptime: number;
  checks: Record<string, boolean>;
}

interface ScriptResult {
  exitCode: number;
  stdout: string;
  stderr: string;
  executionTimeMs: number;
}

interface ApiError {
  error: string;
  message: string;
  code?: string;
}

// ============================================================================
// HyperMachine Client
// ============================================================================

class HyperMachineClient {
  private baseUrl: string;

  constructor(baseUrl: string = "http://localhost:8080") {
    this.baseUrl = baseUrl.replace(/\/$/, "");
  }

  private async request<T>(
    method: string,
    path: string,
    body?: unknown
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const options: RequestInit = {
      method,
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
    };

    if (body !== undefined) {
      options.body = JSON.stringify(body);
    }

    const response = await fetch(url, options);

    if (!response.ok) {
      let errorData: ApiError;
      try {
        errorData = await response.json();
      } catch {
        errorData = {
          error: "unknown_error",
          message: `HTTP ${response.status}: ${response.statusText}`,
        };
      }
      throw new HyperMachineError(
        errorData.message,
        response.status,
        errorData.code
      );
    }

    // Handle empty responses (e.g., DELETE)
    const text = await response.text();
    if (!text) {
      return {} as T;
    }

    return JSON.parse(text) as T;
  }

  // Health check
  async health(): Promise<HealthResponse> {
    return this.request<HealthResponse>("GET", "/health");
  }

  // VM Operations
  async createVm(config: VmConfig): Promise<Vm> {
    return this.request<Vm>("POST", "/api/v1/vms", {
      name: config.name,
      cpus: config.cpus ?? 2,
      memory_mb: config.memoryMb ?? 2048,
      disk_path: config.diskPath,
      network_enabled: config.networkEnabled ?? true,
      metadata: config.metadata ?? {},
    });
  }

  async listVms(): Promise<Vm[]> {
    const response = await this.request<{ vms: Vm[] }>("GET", "/api/v1/vms");
    return response.vms;
  }

  async getVm(vmId: string): Promise<Vm> {
    return this.request<Vm>("GET", `/api/v1/vms/${vmId}`);
  }

  async startVm(vmId: string): Promise<Vm> {
    return this.request<Vm>("POST", `/api/v1/vms/${vmId}/start`);
  }

  async stopVm(vmId: string): Promise<Vm> {
    return this.request<Vm>("POST", `/api/v1/vms/${vmId}/stop`);
  }

  async deleteVm(vmId: string): Promise<void> {
    await this.request<void>("DELETE", `/api/v1/vms/${vmId}`);
  }

  async executeScript(
    vmId: string,
    code: string,
    language: string = "rhai"
  ): Promise<ScriptResult> {
    return this.request<ScriptResult>("POST", `/api/v1/vms/${vmId}/script`, {
      code,
      language,
    });
  }
}

// ============================================================================
// Custom Error Class
// ============================================================================

class HyperMachineError extends Error {
  public statusCode: number;
  public code?: string;

  constructor(message: string, statusCode: number, code?: string) {
    super(message);
    this.name = "HyperMachineError";
    this.statusCode = statusCode;
    this.code = code;
  }
}

// ============================================================================
// Helper Functions
// ============================================================================

function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let unitIndex = 0;
  let value = bytes;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex++;
  }

  return `${value.toFixed(2)} ${units[unitIndex]}`;
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);

  const parts: string[] = [];
  if (days > 0) parts.push(`${days}d`);
  if (hours > 0) parts.push(`${hours}h`);
  if (minutes > 0) parts.push(`${minutes}m`);
  if (secs > 0 || parts.length === 0) parts.push(`${secs}s`);

  return parts.join(" ");
}

// ============================================================================
// Main Example
// ============================================================================

async function main(): Promise<void> {
  console.log("HyperMachine TypeScript SDK Example");
  console.log("=".repeat(50));

  const client = new HyperMachineClient();
  let createdVmId: string | null = null;

  try {
    // Health check
    console.log("\n1. Checking API health...");
    const health = await client.health();
    console.log(`   Status: ${health.status}`);
    console.log(`   Version: ${health.version}`);
    console.log(`   Uptime: ${formatUptime(health.uptime)}`);

    // Create a VM
    console.log("\n2. Creating a new VM...");
    const vm = await client.createVm({
      name: "typescript-example-vm",
      cpus: 2,
      memoryMb: 2048,
      networkEnabled: true,
      metadata: {
        created_by: "typescript-sdk",
        environment: "development",
      },
    });
    createdVmId = vm.id;
    console.log(`   Created VM: ${vm.name} (${vm.id})`);
    console.log(`   CPUs: ${vm.cpus}, Memory: ${formatBytes(vm.memoryMb * 1024 * 1024)}`);

    // List VMs
    console.log("\n3. Listing all VMs...");
    const vms = await client.listVms();
    console.log(`   Found ${vms.length} VM(s):`);
    for (const v of vms) {
      console.log(`   - ${v.name} (${v.state})`);
    }

    // Start VM
    console.log("\n4. Starting VM...");
    const startedVm = await client.startVm(vm.id);
    console.log(`   VM state: ${startedVm.state}`);

    // Execute a script
    console.log("\n5. Executing script in VM...");
    const result = await client.executeScript(
      vm.id,
      `
        let message = "Hello from TypeScript SDK!";
        print(message);
        message
      `,
      "rhai"
    );
    console.log(`   Exit code: ${result.exitCode}`);
    console.log(`   Output: ${result.stdout.trim()}`);
    console.log(`   Execution time: ${result.executionTimeMs}ms`);

    // Stop VM
    console.log("\n6. Stopping VM...");
    const stoppedVm = await client.stopVm(vm.id);
    console.log(`   VM state: ${stoppedVm.state}`);

    // Delete VM
    console.log("\n7. Deleting VM...");
    await client.deleteVm(vm.id);
    createdVmId = null;
    console.log("   VM deleted successfully");

    console.log("\n" + "=".repeat(50));
    console.log("Example completed successfully!");
  } catch (error) {
    if (error instanceof HyperMachineError) {
      console.error(`\nAPI Error (${error.statusCode}): ${error.message}`);
      if (error.code) {
        console.error(`Error code: ${error.code}`);
      }
    } else if (error instanceof TypeError && error.message.includes("fetch")) {
      console.error("\nConnection Error: Could not connect to HyperMachine API");
      console.error("Make sure the server is running at http://localhost:8080");
    } else {
      throw error;
    }
  } finally {
    // Cleanup: delete VM if it was created but not deleted
    if (createdVmId) {
      try {
        console.log("\nCleaning up...");
        await client.deleteVm(createdVmId);
        console.log("Cleanup VM deleted");
      } catch {
        // Ignore cleanup errors
      }
    }
  }
}

// Run the example
main().catch(console.error);
