# 🔧 LightSpeed Tools Registry

> Tool stubs, MCP wrappers, and API interfaces for the LightSpeed WAT system.
> Each tool has an XML-style interface, parameter schema, return format, and error handling.

---

## Tool Index

| ID | Name | Type | MCP Server | Agent Users |
|----|------|------|------------|-------------|
| T-001 | CodeGen | LLM Tool | Native | RustDev, DevOps |
| T-002 | OracleCloudAPI | MCP Wrapper | oracle-cloud-mcp | InfraDev |
| T-003 | PingBenchmark | CLI Tool | ping-monitor-mcp | QAEngineer, NetEng |
| T-004 | PacketAnalyzer | CLI Tool | Native | NetEng, RustDev |
| T-005 | BGPLookingGlass | API Wrapper | Native | NetEng |
| T-006 | GitOps | MCP Wrapper | github-mcp | DevOps, All |
| T-007 | DockerDeploy | CLI Tool | Native | DevOps |
| T-008 | TerraformPlan | CLI Tool | Native | InfraDev |
| T-009 | ShellExec | CLI Tool | Native | All |
| T-010 | DataPipeline | Custom | Native | AIResearcher |
| T-011 | FileManager | MCP Wrapper | filesystem-mcp | All |
| T-012 | WebSearch | API Wrapper | Native | AIResearcher, BizDev |
| T-013 | LatencyProbe | Custom | ping-monitor-mcp | NetEng, QAEngineer |
| T-014 | ModelTrain | Custom | Native | AIResearcher |
| T-015 | CostMonitor | Custom | oracle-cloud-mcp | InfraDev |

---

## Tool Definitions

### T-001: CodeGen

> Generate code using LLM capabilities. Primary tool for RustDev agent.

**Interface:**
```xml
<tool_call name="CodeGen">
  <param name="language">rust|python|hcl|yaml|toml|markdown</param>
  <param name="module">module_name</param>
  <param name="spec">Detailed specification of what to generate</param>
  <param name="output_path">relative/path/to/output</param>
  <param name="dependencies">[optional] crate or package list</param>
  <param name="tests">[optional] true|false - generate tests</param>
  <param name="style">[optional] idiomatic|minimal|verbose</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="CodeGen" status="success|error">
  <files>
    <file path="relative/path">
      [generated code content]
    </file>
  </files>
  <dependencies_added>[list of new dependencies]</dependencies_added>
  <notes>[any implementation notes or warnings]</notes>
</tool_result>
```

**Error Handling:**
```xml
<tool_result name="CodeGen" status="error">
  <error code="SPEC_UNCLEAR|DEPENDENCY_CONFLICT|GENERATION_FAILED">
    [error description]
  </error>
  <suggestion>[how to fix]</suggestion>
</tool_result>
```

**Rules Applied:** `[QUALITY_STUB]`, `[SAFETY_STUB]`

**Example:**
```xml
<tool_call name="CodeGen">
  <param name="language">rust</param>
  <param name="module">tunnel_header</param>
  <param name="spec">
    Implement encode/decode for the LightSpeed tunnel header:
    - Version (4 bits), Flags (4 bits), Hop Count (8 bits), Sequence (16 bits)
    - Timestamp microseconds (32 bits)
    - Original source/dest IP (32 bits each)
    - Original source/dest port (16 bits each), Payload length (16 bits)
    Use bytes crate for zero-copy. Implement From/Into traits.
    Include unit tests for round-trip encode/decode.
  </param>
  <param name="output_path">client/src/tunnel/header.rs</param>
  <param name="dependencies">["bytes"]</param>
  <param name="tests">true</param>
</tool_call>
```

---

### T-002: OracleCloudAPI

> Interact with Oracle Cloud Infrastructure API. MCP-wrapped for automation.

**Interface:**
```xml
<tool_call name="OracleCloudAPI">
  <param name="action">list_regions|create_instance|list_instances|get_instance|terminate_instance|create_vcn|list_vcns|create_subnet|create_security_list|get_usage</param>
  <param name="region">[OCI region identifier]</param>
  <param name="compartment_id">[OCID of compartment]</param>
  <param name="params">{JSON object with action-specific parameters}</param>
</tool_call>
```

**Action Parameters:**

| Action | Required Params | Description |
|--------|----------------|-------------|
| `list_regions` | none | List available regions |
| `create_instance` | `shape`, `image_id`, `subnet_id`, `display_name` | Create compute instance |
| `list_instances` | `compartment_id` | List all instances |
| `get_instance` | `instance_id` | Get instance details |
| `create_vcn` | `cidr_block`, `display_name` | Create virtual cloud network |
| `get_usage` | `compartment_id` | Get free tier usage stats |

**Return Schema:**
```xml
<tool_result name="OracleCloudAPI" status="success|error">
  <data>[JSON response from OCI API]</data>
  <cost_impact>$0.00 (Always Free)</cost_impact>
  <free_tier_usage>
    <resource name="[resource]" used="[N]" limit="[M]" pct="[%]"/>
  </free_tier_usage>
</tool_result>
```

**Safety Gate:**
```
[COST_STUB] ENFORCEMENT:
Before ANY create/modify operation:
1. Verify resource is Always Free eligible
2. Check current usage vs. free tier limits
3. REJECT if operation would exceed free tier
4. Log cost impact assessment
```

**Rules Applied:** `[COST_STUB]` (CRITICAL), `[SECURITY_STUB]`, `[SAFETY_STUB]`

---

### T-003: PingBenchmark

> Measure and compare latency with and without tunnel.

**Interface:**
```xml
<tool_call name="PingBenchmark">
  <param name="mode">direct|tunnel|tunnel_vs_direct|multipath</param>
  <param name="target">IP address or hostname of game server</param>
  <param name="proxy">[optional] proxy node to tunnel through</param>
  <param name="count">[optional, default 100] number of pings</param>
  <param name="interval_ms">[optional, default 100] interval between pings</param>
  <param name="duration">[optional] duration in seconds (overrides count)</param>
  <param name="output">path/to/results.json</param>
  <param name="game">[optional] fortnite|cs2|dota2 - use game-specific ports</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="PingBenchmark" status="success|error">
  <results>
    <direct>
      <p50_ms>[value]</p50_ms>
      <p95_ms>[value]</p95_ms>
      <p99_ms>[value]</p99_ms>
      <jitter_ms>[value]</jitter_ms>
      <packet_loss_pct>[value]</packet_loss_pct>
    </direct>
    <tunnel proxy="[node]">
      <p50_ms>[value]</p50_ms>
      <p95_ms>[value]</p95_ms>
      <p99_ms>[value]</p99_ms>
      <jitter_ms>[value]</jitter_ms>
      <packet_loss_pct>[value]</packet_loss_pct>
    </tunnel>
    <improvement>
      <p50_delta_ms>[value]</p50_delta_ms>
      <p50_delta_pct>[value]%</p50_delta_pct>
    </improvement>
  </results>
  <saved_to>[output path]</saved_to>
</tool_result>
```

**Rules Applied:** `[QUALITY_STUB]`, `[ETHICS_STUB]` (honest measurements)

---

### T-004: PacketAnalyzer

> Capture and analyze network packets for protocol development and debugging.

**Interface:**
```xml
<tool_call name="PacketAnalyzer">
  <param name="action">capture|analyze|filter|stats</param>
  <param name="interface">[network interface name]</param>
  <param name="filter">[BPF filter expression, e.g., "udp port 7777"]</param>
  <param name="count">[number of packets to capture]</param>
  <param name="duration">[capture duration in seconds]</param>
  <param name="output">path/to/capture.pcap</param>
  <param name="analyze_fields">[optional] src_ip|dst_ip|ports|payload_size|timing</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="PacketAnalyzer" status="success|error">
  <capture_stats>
    <packets_captured>[N]</packets_captured>
    <duration_sec>[T]</duration_sec>
    <avg_packet_size>[bytes]</avg_packet_size>
    <protocols>
      <protocol name="UDP" count="[N]" pct="[%]"/>
    </protocols>
  </capture_stats>
  <analysis>[detailed analysis if action=analyze]</analysis>
  <saved_to>[output path]</saved_to>
</tool_result>
```

**Rules Applied:** `[PRIVACY_STUB]` (no payload logging), `[SECURITY_STUB]`

---

### T-005: BGPLookingGlass

> Query public BGP looking glass servers for route analysis.

**Interface:**
```xml
<tool_call name="BGPLookingGlass">
  <param name="action">route|traceroute|as_path|prefix</param>
  <param name="target">IP address or prefix</param>
  <param name="source">[optional] looking glass location</param>
  <param name="format">raw|parsed|json</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="BGPLookingGlass" status="success|error">
  <route>
    <prefix>[IP prefix]</prefix>
    <as_path>[AS number sequence]</as_path>
    <next_hop>[next hop IP]</next_hop>
    <origin>[IGP|EGP|Incomplete]</origin>
    <communities>[community strings]</communities>
  </route>
  <analysis>
    <path_length>[number of AS hops]</path_length>
    <optimization_potential>[assessment]</optimization_potential>
  </analysis>
</tool_result>
```

**Public Looking Glass Sources:**
- RIPE RIS: `https://stat.ripe.net/`
- RouteViews: `http://www.routeviews.org/`
- Hurricane Electric: `https://lg.he.net/`
- PCH: `https://www.pch.net/tools/looking_glass`

**Rules Applied:** `[ETHICS_STUB]` (respect rate limits), `[TRANSPARENCY_STUB]`

---

### T-006: GitOps

> GitHub operations via the GitHub MCP server. Used for repo management, PRs, issues, releases.

**Interface:**
```xml
<tool_call name="GitOps">
  <param name="action">create_file|push_files|create_pr|create_issue|create_release|list_issues|get_file</param>
  <param name="repo">owner/repo</param>
  <param name="branch">[branch name]</param>
  <param name="params">{action-specific JSON parameters}</param>
</tool_call>
```

**Maps to MCP Tools:**
| GitOps Action | MCP Tool |
|--------------|----------|
| `create_file` | `create_or_update_file` |
| `push_files` | `push_files` |
| `create_pr` | `create_pull_request` |
| `create_issue` | `issue_write` (method: create) |
| `create_release` | GitHub API via MCP |
| `list_issues` | `list_issues` |
| `get_file` | `get_file_contents` |

**Rules Applied:** `[SECURITY_STUB]` (no secrets in commits), `[QUALITY_STUB]`

---

### T-007: DockerDeploy

> Build and deploy Docker containers to target infrastructure.

**Interface:**
```xml
<tool_call name="DockerDeploy">
  <param name="action">build|push|deploy|status|logs|restart</param>
  <param name="image">[image:tag]</param>
  <param name="dockerfile">[path to Dockerfile]</param>
  <param name="targets">["node-1", "node-2"]</param>
  <param name="env">{environment variables}</param>
  <param name="ports">["8080:8080/udp", "443:443/tcp"]</param>
  <param name="health_check">[health check endpoint]</param>
  <param name="restart_policy">always|on-failure|never</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="DockerDeploy" status="success|error">
  <deployments>
    <node name="[node]" status="running|failed" image="[image:tag]">
      <health>[healthy|unhealthy|pending]</health>
      <uptime>[duration]</uptime>
    </node>
  </deployments>
</tool_result>
```

**Rules Applied:** `[COST_STUB]`, `[SECURITY_STUB]` (no root containers)

---

### T-008: TerraformPlan

> Execute Terraform operations for infrastructure management.

**Interface:**
```xml
<tool_call name="TerraformPlan">
  <param name="action">init|plan|apply|destroy|output|state</param>
  <param name="directory">[terraform directory path]</param>
  <param name="var_file">[optional .tfvars file]</param>
  <param name="target">[optional specific resource]</param>
  <param name="auto_approve">[optional, default false] true for apply</param>
</tool_call>
```

**Safety Gate for Apply/Destroy:**
```
BEFORE terraform apply/destroy:
1. Run terraform plan first
2. Verify ALL resources are Always Free tier [COST_STUB]
3. Display plan summary for review
4. Require explicit confirmation (auto_approve=true)
5. Log operation to state/decisions.md
```

**Return Schema:**
```xml
<tool_result name="TerraformPlan" status="success|error">
  <plan_summary>
    <add>[N]</add>
    <change>[N]</change>
    <destroy>[N]</destroy>
  </plan_summary>
  <resources>[list of affected resources]</resources>
  <cost_assessment>$0.00 (all Always Free tier)</cost_assessment>
  <output>[terraform output]</output>
</tool_result>
```

**Rules Applied:** `[COST_STUB]` (CRITICAL), `[SAFETY_STUB]`, `[SECURITY_STUB]`

---

### T-009: ShellExec

> Execute shell commands on local or remote machines.

**Interface:**
```xml
<tool_call name="ShellExec">
  <param name="command">[shell command]</param>
  <param name="target">local|[remote-host]</param>
  <param name="working_dir">[optional working directory]</param>
  <param name="timeout">[optional timeout in seconds, default 60]</param>
  <param name="env">{optional environment variables}</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="ShellExec" status="success|error">
  <stdout>[standard output]</stdout>
  <stderr>[standard error]</stderr>
  <exit_code>[0-255]</exit_code>
  <duration_ms>[execution time]</duration_ms>
</tool_result>
```

**Safety Gate:**
```
[SAFETY_STUB] ENFORCEMENT:
BLOCKED commands (require human approval):
- rm -rf / (or similar destructive)
- Any command modifying system files
- Network scanning (nmap without explicit scope)
- Commands with sudo that affect system config

ALLOWED without approval:
- cargo build/test/run
- docker build/run
- terraform plan (not apply)
- ping/traceroute/mtr
- cat/ls/grep/find
- git operations
```

**Rules Applied:** `[SAFETY_STUB]` (CRITICAL), `[SECURITY_STUB]`

---

### T-010: DataPipeline

> Collect, store, and process latency measurement data for ML training.

**Interface:**
```xml
<tool_call name="DataPipeline">
  <param name="action">collect|store|query|export|schema</param>
  <param name="source">[proxy-nodes|client|benchmark]</param>
  <param name="destination">[file path or database]</param>
  <param name="format">csv|json|parquet</param>
  <param name="filters">{optional filters: time_range, game, region}</param>
  <param name="aggregation">[optional: minute|hour|day]</param>
</tool_call>
```

**Data Schema:**
```json
{
  "timestamp": "ISO8601",
  "source_region": "string",
  "dest_region": "string",
  "proxy_node": "string",
  "game": "string",
  "latency_ms": "float",
  "jitter_ms": "float",
  "packet_loss_pct": "float",
  "hop_count": "int",
  "route_hash": "string",
  "time_of_day_bucket": "string",
  "day_of_week": "int"
}
```

**Rules Applied:** `[PRIVACY_STUB]` (no PII), `[COST_STUB]` (local storage only)

---

### T-011: FileManager

> Read, write, and manage local files. Maps to filesystem MCP operations.

**Interface:**
```xml
<tool_call name="FileManager">
  <param name="action">read|write|append|delete|list|search|move</param>
  <param name="path">[file or directory path]</param>
  <param name="content">[content for write/append]</param>
  <param name="pattern">[search pattern for search action]</param>
  <param name="recursive">[optional, default false]</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="FileManager" status="success|error">
  <content>[file content for read]</content>
  <files>[file list for list/search]</files>
  <written>[bytes written for write/append]</written>
</tool_result>
```

**Rules Applied:** `[SAFETY_STUB]` (no system file modification)

---

### T-012: WebSearch

> Search the web for research, documentation, and competitive intelligence.

**Interface:**
```xml
<tool_call name="WebSearch">
  <param name="query">[search query]</param>
  <param name="domain">[optional domain filter]</param>
  <param name="type">general|technical|academic|news</param>
  <param name="max_results">[optional, default 10]</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="WebSearch" status="success|error">
  <results>
    <result rank="[N]">
      <title>[result title]</title>
      <url>[result URL]</url>
      <snippet>[relevant snippet]</snippet>
    </result>
  </results>
</tool_result>
```

**Rules Applied:** `[ETHICS_STUB]` (respect robots.txt), `[PRIVACY_STUB]`

---

### T-013: LatencyProbe

> Active latency probing tool for continuous measurement from proxy nodes.

**Interface:**
```xml
<tool_call name="LatencyProbe">
  <param name="action">start|stop|status|results</param>
  <param name="probe_id">[unique probe identifier]</param>
  <param name="source">[proxy node or local]</param>
  <param name="targets">["game-server-1:port", "game-server-2:port"]</param>
  <param name="protocol">udp|tcp|icmp</param>
  <param name="interval_ms">[probe interval, default 1000]</param>
  <param name="window">[rolling window for stats, default 300s]</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="LatencyProbe" status="success|error">
  <probe id="[probe_id]" status="running|stopped">
    <target addr="[target]">
      <current_ms>[latest]</current_ms>
      <p50_ms>[value]</p50_ms>
      <p95_ms>[value]</p95_ms>
      <jitter_ms>[value]</jitter_ms>
      <loss_pct>[value]</loss_pct>
      <samples>[count]</samples>
    </target>
  </probe>
</tool_result>
```

**Rules Applied:** `[ETHICS_STUB]` (reasonable probe rates), `[SAFETY_STUB]`

---

### T-014: ModelTrain

> Train and evaluate ML models using linfa or other Rust ML frameworks.

**Interface:**
```xml
<tool_call name="ModelTrain">
  <param name="action">train|evaluate|predict|export|import</param>
  <param name="model_type">random_forest|gradient_boost|linear_regression|knn</param>
  <param name="data_path">[path to training data]</param>
  <param name="features">["feature1", "feature2", ...]</param>
  <param name="target">[target variable name]</param>
  <param name="split_ratio">[optional, default 0.8]</param>
  <param name="hyperparams">{optional model-specific hyperparameters}</param>
  <param name="output_model">[path to save trained model]</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="ModelTrain" status="success|error">
  <model type="[type]" path="[saved path]">
    <metrics>
      <mae>[value]</mae>
      <rmse>[value]</rmse>
      <r_squared>[value]</r_squared>
      <accuracy>[value for classification]</accuracy>
    </metrics>
    <feature_importance>
      <feature name="[name]" importance="[value]"/>
    </feature_importance>
    <inference_time_us>[microseconds per prediction]</inference_time_us>
  </model>
</tool_result>
```

**Rules Applied:** `[QUALITY_STUB]` (cross-validation required), `[COST_STUB]` (local training only)

---

### T-015: CostMonitor

> Monitor Oracle Cloud free tier usage to prevent accidental billing.

**Interface:**
```xml
<tool_call name="CostMonitor">
  <param name="action">check|alert|report|forecast</param>
  <param name="compartment_id">[OCI compartment]</param>
  <param name="threshold_pct">[optional alert threshold, default 80]</param>
</tool_call>
```

**Return Schema:**
```xml
<tool_result name="CostMonitor" status="success|warning|critical">
  <resources>
    <resource name="Compute OCPU" used="2" limit="4" pct="50%" status="ok"/>
    <resource name="Memory GB" used="12" limit="24" pct="50%" status="ok"/>
    <resource name="Storage GB" used="100" limit="200" pct="50%" status="ok"/>
    <resource name="Outbound TB" used="3" limit="10" pct="30%" status="ok"/>
  </resources>
  <total_cost>$0.00</total_cost>
  <forecast_cost>$0.00</forecast_cost>
  <alerts>[any threshold breaches]</alerts>
</tool_result>
```

**Rules Applied:** `[COST_STUB]` (PRIMARY — this tool enforces cost rules)

---

## Tool Composition Patterns

### Sequential Composition
```xml
<!-- Step 1: Generate code -->
<tool_call name="CodeGen">
  <param name="language">rust</param>
  <param name="module">tunnel</param>
  <param name="spec">[spec]</param>
  <param name="output_path">client/src/tunnel/</param>
</tool_call>

<!-- Step 2: Build and test (depends on Step 1 output) -->
<tool_call name="ShellExec">
  <param name="command">cd client && cargo test</param>
  <param name="target">local</param>
</tool_call>

<!-- Step 3: Benchmark (depends on Step 2 success) -->
<tool_call name="PingBenchmark">
  <param name="mode">tunnel_vs_direct</param>
  <param name="target">game-server</param>
  <param name="output">tests/results/tunnel-bench.json</param>
</tool_call>
```

### Parallel Composition
```xml
<!-- These can run simultaneously -->
<parallel>
  <tool_call name="PingBenchmark">
    <param name="target">fortnite-us-east</param>
    <param name="game">fortnite</param>
  </tool_call>

  <tool_call name="PingBenchmark">
    <param name="target">cs2-eu-west</param>
    <param name="game">cs2</param>
  </tool_call>

  <tool_call name="PingBenchmark">
    <param name="target">dota2-sea</param>
    <param name="game">dota2</param>
  </tool_call>
</parallel>
```

### Conditional Composition
```xml
<tool_call name="CostMonitor">
  <param name="action">check</param>
</tool_call>

<!-- IF CostMonitor status="ok" THEN proceed -->
<conditional on="CostMonitor.status == ok">
  <tool_call name="OracleCloudAPI">
    <param name="action">create_instance</param>
    <param name="params">{...}</param>
  </tool_call>
</conditional>

<!-- ELSE abort and alert -->
<conditional on="CostMonitor.status != ok">
  <alert level="critical">Free tier limit approaching. Aborting instance creation.</alert>
</conditional>
```

---

## Custom Tool Development Guide

### Creating a New Tool

1. **Define the interface** using the XML schema above
2. **Implement** as one of:
   - **LLM Tool**: Handled by the LLM directly (like CodeGen)
   - **CLI Tool**: Wraps a command-line program
   - **API Wrapper**: Calls an external API
   - **MCP Server**: Full MCP server implementation (see `mcp-integration.md`)
3. **Register** in this file with ID, name, type, and agent users
4. **Add rules**: Specify which rule stubs apply
5. **Test**: Verify with sample inputs/outputs

### Tool Template
```xml
<!-- T-XXX: [ToolName] -->
<tool_definition id="T-XXX" name="[ToolName]">
  <description>[What this tool does]</description>
  <type>[LLM Tool|CLI Tool|API Wrapper|MCP Server]</type>
  <agents>[comma-separated agent list]</agents>
  <rules>[applicable rule stubs]</rules>

  <interface>
    <param name="action" required="true">[actions]</param>
    <param name="[param]" required="[true|false]">[description]</param>
  </interface>

  <returns>
    <field name="[field]" type="[type]">[description]</field>
  </returns>

  <errors>
    <error code="[CODE]">[description]</error>
  </errors>
</tool_definition>
```
