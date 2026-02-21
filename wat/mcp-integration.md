# 🔌 LightSpeed MCP Integration

> Model Context Protocol server specifications, configurations, and custom server designs.
> MCP enables Claude and other LLMs to interact with external tools, APIs, and resources.

---

## MCP Server Index

| ID | Server Name | Type | Status | Purpose |
|----|-------------|------|--------|---------|
| MCP-001 | `github-mcp` | Pre-built | ✅ Active | GitHub repo/PR/issue management |
| MCP-002 | `filesystem-mcp` | Pre-built | ✅ Active | Local file operations |
| MCP-003 | `oracle-cloud-mcp` | Custom Build | 🔧 Planned | OCI API automation |
| MCP-004 | `ping-monitor-mcp` | Custom Build | 🔧 Planned | Latency monitoring |
| MCP-005 | `deploy-pipeline-mcp` | Custom Build | 🔧 Planned | Deployment automation |
| MCP-006 | `proxy-manager-mcp` | Custom Build | 🔧 Planned | Proxy node management |

---

## MCP-001: GitHub MCP Server

> **Status: Active** — Already connected and available.

### Configuration

```json
{
  "mcpServers": {
    "github": {
      "command": "github-mcp-server",
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "[GITHUB_TOKEN]"
      }
    }
  }
}
```

### Available Tools

| Tool | WAT Mapping | Used By |
|------|-------------|---------|
| `create_or_update_file` | GitOps → create_file | DevOps, RustDev |
| `push_files` | GitOps → push_files | DevOps |
| `create_pull_request` | GitOps → create_pr | DevOps |
| `issue_write` | GitOps → create_issue | ProductManager, QAEngineer |
| `list_issues` | GitOps → list_issues | ProductManager |
| `get_file_contents` | GitOps → get_file | All |
| `search_code` | Code search | RustDev, NetEng |
| `create_branch` | Branch management | DevOps |
| `list_commits` | Commit history | DevOps |

### WAT Integration Examples

**Creating a file in the repo:**
```xml
<tool_call name="GitOps">
  <param name="action">create_file</param>
  <param name="repo">lightspeed/lightspeed</param>
  <param name="branch">main</param>
  <param name="params">{
    "path": "client/src/tunnel/header.rs",
    "content": "[generated Rust code]",
    "message": "feat(tunnel): add tunnel header encode/decode"
  }</param>
</tool_call>
```

**Mapping to actual MCP call:**
```
MCP Tool: create_or_update_file
Parameters:
  owner: "lightspeed"
  repo: "lightspeed"
  branch: "main"
  path: "client/src/tunnel/header.rs"
  content: "[generated Rust code]"
  message: "feat(tunnel): add tunnel header encode/decode"
```

### Available Resources

| Resource URI | Description |
|-------------|-------------|
| `github://repo/lightspeed/lightspeed` | Main repository |
| `github://issues/lightspeed/lightspeed` | Issue tracker |
| `github://pulls/lightspeed/lightspeed` | Pull requests |

---

## MCP-002: Filesystem MCP Server

> **Status: Active** — Available for local file operations.

### Configuration

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/lightspeed"]
    }
  }
}
```

### Available Tools

| Tool | WAT Mapping | Used By |
|------|-------------|---------|
| `read_file` | FileManager → read | All |
| `write_file` | FileManager → write | All |
| `list_directory` | FileManager → list | All |
| `search_files` | FileManager → search | All |
| `move_file` | FileManager → move | All |

### WAT Integration

All `FileManager` tool calls (T-011) map directly to filesystem MCP operations:

```xml
<!-- WAT Tool Call -->
<tool_call name="FileManager">
  <param name="action">write</param>
  <param name="path">wat/state/current-phase.md</param>
  <param name="content">[state content]</param>
</tool_call>

<!-- Maps to MCP -->
MCP Tool: write_file
Parameters:
  path: "wat/state/current-phase.md"
  content: "[state content]"
```

---

## MCP-003: Oracle Cloud MCP Server (Custom)

> **Status: Planned** — To be built as part of WF-002.

### Purpose

Automate Oracle Cloud Infrastructure operations for proxy node management while enforcing free-tier cost constraints.

### Server Specification

```json
{
  "mcpServers": {
    "oracle-cloud": {
      "command": "node",
      "args": ["mcp-servers/oracle-cloud/index.js"],
      "env": {
        "OCI_TENANCY_OCID": "[TENANCY_OCID]",
        "OCI_USER_OCID": "[USER_OCID]",
        "OCI_FINGERPRINT": "[API_KEY_FINGERPRINT]",
        "OCI_PRIVATE_KEY_PATH": "[PATH_TO_PEM]",
        "OCI_REGION": "[HOME_REGION]"
      }
    }
  }
}
```

### Tool Definitions

#### `oci_list_instances`
```json
{
  "name": "oci_list_instances",
  "description": "List all compute instances in a compartment. Returns instance details including status, shape, and free-tier eligibility.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "compartment_id": {
        "type": "string",
        "description": "OCID of the compartment"
      },
      "region": {
        "type": "string",
        "description": "OCI region identifier (e.g., us-ashburn-1)"
      }
    },
    "required": ["compartment_id"]
  }
}
```

#### `oci_create_instance`
```json
{
  "name": "oci_create_instance",
  "description": "Create a new Always Free tier compute instance. ENFORCES free tier limits — will reject if limits would be exceeded.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "compartment_id": { "type": "string" },
      "display_name": { "type": "string" },
      "shape": {
        "type": "string",
        "enum": ["VM.Standard.A1.Flex"],
        "description": "Must be Always Free eligible shape"
      },
      "ocpus": {
        "type": "number",
        "maximum": 4,
        "description": "Number of OCPUs (max 4 across all instances)"
      },
      "memory_gb": {
        "type": "number",
        "maximum": 24,
        "description": "Memory in GB (max 24 across all instances)"
      },
      "subnet_id": { "type": "string" },
      "image_id": { "type": "string" },
      "ssh_public_key": { "type": "string" }
    },
    "required": ["compartment_id", "display_name", "shape", "ocpus", "memory_gb", "subnet_id", "image_id"]
  }
}
```

#### `oci_get_usage`
```json
{
  "name": "oci_get_usage",
  "description": "Get current free tier resource usage. Returns usage vs limits for all Always Free resources.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "compartment_id": { "type": "string" }
    },
    "required": ["compartment_id"]
  }
}
```

#### `oci_create_vcn`
```json
{
  "name": "oci_create_vcn",
  "description": "Create a Virtual Cloud Network. Limited to 2 VCNs on free tier.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "compartment_id": { "type": "string" },
      "display_name": { "type": "string" },
      "cidr_blocks": {
        "type": "array",
        "items": { "type": "string" }
      }
    },
    "required": ["compartment_id", "display_name", "cidr_blocks"]
  }
}
```

#### `oci_manage_security_list`
```json
{
  "name": "oci_manage_security_list",
  "description": "Create or update security list rules for a VCN subnet.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "action": {
        "type": "string",
        "enum": ["create", "update", "get"]
      },
      "compartment_id": { "type": "string" },
      "vcn_id": { "type": "string" },
      "display_name": { "type": "string" },
      "ingress_rules": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "protocol": { "type": "string" },
            "source": { "type": "string" },
            "port_range": { "type": "string" }
          }
        }
      },
      "egress_rules": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "protocol": { "type": "string" },
            "destination": { "type": "string" }
          }
        }
      }
    },
    "required": ["action", "compartment_id"]
  }
}
```

### Resource Definitions

```json
{
  "resources": [
    {
      "uri": "oci://usage/free-tier",
      "name": "Free Tier Usage",
      "description": "Current usage of Oracle Cloud Always Free resources",
      "mimeType": "application/json"
    },
    {
      "uri": "oci://instances/list",
      "name": "Instance List",
      "description": "List of all compute instances",
      "mimeType": "application/json"
    },
    {
      "uri": "oci://network/topology",
      "name": "Network Topology",
      "description": "VCNs, subnets, and security lists",
      "mimeType": "application/json"
    }
  ]
}
```

### Implementation Plan

```
Language: Node.js (TypeScript)
Framework: @modelcontextprotocol/sdk
Dependencies: oci-sdk (Oracle Cloud SDK for Node.js)
Location: mcp-servers/oracle-cloud/

Files:
├── index.ts          # Server entry point
├── tools/
│   ├── instances.ts  # Compute instance tools
│   ├── networking.ts # VCN/subnet tools
│   └── usage.ts      # Free tier usage monitoring
├── resources/
│   ├── usage.ts      # Free tier usage resource
│   └── topology.ts   # Network topology resource
├── guards/
│   └── cost.ts       # Cost guard — enforces free tier limits
├── package.json
└── tsconfig.json
```

### Cost Guard (Critical)

Every mutating operation passes through the cost guard:

```typescript
// guards/cost.ts (pseudocode)
async function costGuard(operation: OciOperation): Promise<boolean> {
  const currentUsage = await getFreeTierUsage();

  const projectedUsage = calculateProjected(currentUsage, operation);

  for (const resource of projectedUsage) {
    if (resource.usagePercent > 90) {
      throw new CostLimitError(
        `Operation would exceed 90% of free tier limit for ${resource.name}: ` +
        `${resource.projected}/${resource.limit} (${resource.usagePercent}%)`
      );
    }
  }

  // Log the approved operation
  await logCostDecision(operation, currentUsage, projectedUsage);
  return true;
}
```

---

## MCP-004: Ping Monitor MCP Server (Custom)

> **Status: Planned** — To be built for continuous latency monitoring.

### Purpose

Provide real-time and historical latency data from proxy nodes to game servers.

### Server Specification

```json
{
  "mcpServers": {
    "ping-monitor": {
      "command": "node",
      "args": ["mcp-servers/ping-monitor/index.js"],
      "env": {
        "PROXY_NODES": "[comma-separated node addresses]",
        "DATA_DIR": "./ai/data/latency"
      }
    }
  }
}
```

### Tool Definitions

#### `ping_measure`
```json
{
  "name": "ping_measure",
  "description": "Measure latency from a source to target with various protocols.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "source": {
        "type": "string",
        "description": "Source node (proxy name or 'local')"
      },
      "target": {
        "type": "string",
        "description": "Target address (IP:port or hostname)"
      },
      "protocol": {
        "type": "string",
        "enum": ["icmp", "udp", "tcp"]
      },
      "count": {
        "type": "number",
        "default": 10,
        "description": "Number of measurements"
      },
      "interval_ms": {
        "type": "number",
        "default": 1000
      }
    },
    "required": ["target"]
  }
}
```

#### `ping_benchmark`
```json
{
  "name": "ping_benchmark",
  "description": "Run a comprehensive latency benchmark comparing direct vs tunneled paths.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "target": { "type": "string" },
      "game": {
        "type": "string",
        "enum": ["fortnite", "cs2", "dota2", "generic"]
      },
      "duration_sec": { "type": "number", "default": 60 },
      "proxies": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Proxy nodes to test through"
      }
    },
    "required": ["target"]
  }
}
```

#### `ping_history`
```json
{
  "name": "ping_history",
  "description": "Query historical latency data for ML training or analysis.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "source": { "type": "string" },
      "target": { "type": "string" },
      "time_range": {
        "type": "object",
        "properties": {
          "start": { "type": "string", "format": "date-time" },
          "end": { "type": "string", "format": "date-time" }
        }
      },
      "aggregation": {
        "type": "string",
        "enum": ["raw", "minute", "hour", "day"]
      },
      "format": {
        "type": "string",
        "enum": ["json", "csv"]
      }
    },
    "required": ["target"]
  }
}
```

### Resource Definitions

```json
{
  "resources": [
    {
      "uri": "ping://current/{node}",
      "name": "Current Latency",
      "description": "Real-time latency from a specific node to all monitored targets"
    },
    {
      "uri": "ping://dashboard",
      "name": "Latency Dashboard",
      "description": "Overview of all probe results across all nodes"
    },
    {
      "uri": "ping://data/export",
      "name": "Data Export",
      "description": "Export latency data for ML training"
    }
  ]
}
```

### Implementation Plan

```
Language: Node.js (TypeScript) or Rust
Location: mcp-servers/ping-monitor/

Core Components:
├── index.ts           # MCP server entry
├── probes/
│   ├── icmp.ts        # ICMP ping implementation
│   ├── udp.ts         # UDP probe implementation
│   └── tcp.ts         # TCP probe implementation
├── storage/
│   ├── timeseries.ts  # Time-series data storage (SQLite or flat file)
│   └── export.ts      # Data export for ML
├── scheduler/
│   └── cron.ts        # Scheduled probe execution
└── analysis/
    └── stats.ts       # Statistical analysis (p50, p95, jitter)
```

---

## MCP-005: Deploy Pipeline MCP Server (Custom)

> **Status: Planned** — To be built for automated deployment.

### Purpose

Orchestrate deployments of proxy servers and client releases.

### Tool Definitions

#### `deploy_proxy`
```json
{
  "name": "deploy_proxy",
  "description": "Deploy or update proxy server on a target node.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "node": { "type": "string" },
      "version": { "type": "string" },
      "action": {
        "type": "string",
        "enum": ["deploy", "update", "rollback", "restart", "status"]
      },
      "config": {
        "type": "object",
        "description": "Proxy configuration overrides"
      }
    },
    "required": ["node", "action"]
  }
}
```

#### `deploy_release`
```json
{
  "name": "deploy_release",
  "description": "Build and publish a client release.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "version": { "type": "string" },
      "targets": {
        "type": "array",
        "items": {
          "type": "string",
          "enum": ["windows-x64", "linux-x64", "linux-arm64", "macos-x64", "macos-arm64"]
        }
      },
      "channel": {
        "type": "string",
        "enum": ["dev", "beta", "stable"]
      },
      "changelog": { "type": "string" }
    },
    "required": ["version", "targets", "channel"]
  }
}
```

---

## MCP-006: Proxy Manager MCP Server (Custom)

> **Status: Planned** — To be built for runtime proxy management.

### Purpose

Runtime management of the proxy mesh: health, load, routing, configuration.

### Tool Definitions

#### `proxy_health`
```json
{
  "name": "proxy_health",
  "description": "Check health status of proxy nodes.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "node": {
        "type": "string",
        "description": "Specific node or 'all'"
      }
    }
  }
}
```

#### `proxy_config`
```json
{
  "name": "proxy_config",
  "description": "Get or update proxy node configuration.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "node": { "type": "string" },
      "action": {
        "type": "string",
        "enum": ["get", "update"]
      },
      "config": { "type": "object" }
    },
    "required": ["node", "action"]
  }
}
```

#### `proxy_metrics`
```json
{
  "name": "proxy_metrics",
  "description": "Get runtime metrics from proxy nodes.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "node": { "type": "string" },
      "metrics": {
        "type": "array",
        "items": {
          "type": "string",
          "enum": ["connections", "bandwidth", "latency", "errors", "cpu", "memory"]
        }
      },
      "time_range": { "type": "string", "description": "e.g., '1h', '24h', '7d'" }
    },
    "required": ["node"]
  }
}
```

---

## MCP Architecture

### Connection Topology

```
┌─────────────────────────────────────────────────────┐
│                    Claude / LLM                      │
│                 (WAT Autonomy Loop)                  │
└───┬────────┬────────┬────────┬────────┬────────┬────┘
    │        │        │        │        │        │
    ▼        ▼        ▼        ▼        ▼        ▼
┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐
│GitHub│ │ File │ │Oracle│ │ Ping │ │Deploy│ │Proxy │
│ MCP  │ │System│ │Cloud │ │Monitor│ │Pipe  │ │Mgr   │
│Server│ │ MCP  │ │ MCP  │ │ MCP  │ │ MCP  │ │ MCP  │
└──┬───┘ └──┬───┘ └──┬───┘ └──┬───┘ └──┬───┘ └──┬───┘
   │        │        │        │        │        │
   ▼        ▼        ▼        ▼        ▼        ▼
┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐
│GitHub│ │Local │ │Oracle│ │Proxy │ │Build │ │Proxy │
│ API  │ │Files │ │Cloud │ │Nodes │ │System│ │Nodes │
└──────┘ └──────┘ └──────┘ └──────┘ └──────┘ └──────┘
```

### MCP Server Development Standards

1. **Language**: TypeScript (Node.js) preferred for MCP servers
2. **Framework**: `@modelcontextprotocol/sdk` 
3. **Error Handling**: All tools must return structured errors
4. **Logging**: Structured JSON logging via stderr
5. **Testing**: Each tool must have integration tests
6. **Documentation**: Tool descriptions must be clear for LLM consumption

### Claude Desktop Configuration

Complete `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "github": {
      "command": "github-mcp-server",
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "[TOKEN]"
      }
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:/Users/User/Documents/GitHub/lightspeed"]
    },
    "oracle-cloud": {
      "command": "node",
      "args": ["C:/Users/User/Documents/GitHub/lightspeed/mcp-servers/oracle-cloud/dist/index.js"],
      "env": {
        "OCI_CONFIG_FILE": "~/.oci/config"
      }
    },
    "ping-monitor": {
      "command": "node",
      "args": ["C:/Users/User/Documents/GitHub/lightspeed/mcp-servers/ping-monitor/dist/index.js"],
      "env": {
        "DATA_DIR": "C:/Users/User/Documents/GitHub/lightspeed/ai/data/latency"
      }
    },
    "deploy-pipeline": {
      "command": "node",
      "args": ["C:/Users/User/Documents/GitHub/lightspeed/mcp-servers/deploy-pipeline/dist/index.js"]
    },
    "proxy-manager": {
      "command": "node",
      "args": ["C:/Users/User/Documents/GitHub/lightspeed/mcp-servers/proxy-manager/dist/index.js"]
    }
  }
}
```

### Build Order for Custom Servers

```
Phase 1 (WF-002): oracle-cloud-mcp → Required for infrastructure
Phase 2 (WF-001): ping-monitor-mcp → Required for testing
Phase 3 (WF-005): deploy-pipeline-mcp → Required for deployment
Phase 4 (WF-005): proxy-manager-mcp → Required for operations
```

---

## MCP Server Template

Use this template when building new MCP servers:

```typescript
// mcp-servers/[name]/src/index.ts
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
  ListResourcesRequestSchema,
  ReadResourceRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";

const server = new Server(
  { name: "[server-name]-mcp", version: "0.1.0" },
  { capabilities: { tools: {}, resources: {} } }
);

// List available tools
server.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: [
    {
      name: "[tool_name]",
      description: "[Tool description for LLM consumption]",
      inputSchema: {
        type: "object",
        properties: {
          // ... parameters
        },
        required: [/* required params */],
      },
    },
  ],
}));

// Handle tool calls
server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;

  switch (name) {
    case "[tool_name]":
      // Implementation with cost guard if applicable
      return { content: [{ type: "text", text: JSON.stringify(result) }] };
    default:
      throw new Error(`Unknown tool: ${name}`);
  }
});

// Start server
const transport = new StdioServerTransport();
await server.connect(transport);
```

### Package Template

```json
{
  "name": "@lightspeed/[name]-mcp",
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "build": "tsc",
    "start": "node dist/index.js",
    "dev": "tsx src/index.ts"
  },
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.0.0"
  },
  "devDependencies": {
    "typescript": "^5.0.0",
    "tsx": "^4.0.0"
  }
}
```
