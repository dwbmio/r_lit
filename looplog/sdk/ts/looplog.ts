export type LoopLogMeta = Record<string, unknown>;

export interface LoopLogClientOptions {
  endpoint?: string;
  timeoutMs?: number;
}

export interface StartRunInput {
  tag?: string;
  source?: string;
  cwd?: string;
  argv?: string[];
  client_id?: string;
  kind?: string;
  meta?: LoopLogMeta;
}

export interface AppendLineInput {
  ts?: string;
  stream?: string;
  level?: string;
  event?: string;
  text: string;
  meta?: LoopLogMeta;
}

export interface FinishRunInput {
  status?: string;
  exit_code?: number;
}

export class LoopLogClient {
  private endpoint: string;
  private timeoutMs: number;

  constructor(options: LoopLogClientOptions = {}) {
    this.endpoint = (options.endpoint ?? "http://127.0.0.1:3768").replace(/\/+$/, "");
    this.timeoutMs = options.timeoutMs ?? 800;
  }

  async startRun(input: StartRunInput): Promise<LoopLogRun> {
    const result = await this.request<{ run_id: string }>("/v1/runs", {
      method: "POST",
      body: JSON.stringify(input),
      headers: { "content-type": "application/json" },
    });
    return new LoopLogRun(this, result.run_id);
  }

  async appendLines(runId: string, lines: AppendLineInput[]): Promise<void> {
    if (lines.length === 0) return;
    const body = lines.map((line) => JSON.stringify(line)).join("\n") + "\n";
    await this.request(`/v1/runs/${encodeURIComponent(runId)}/lines`, {
      method: "POST",
      body,
      headers: { "content-type": "application/x-ndjson" },
    });
  }

  async finishRun(runId: string, input: FinishRunInput = {}): Promise<void> {
    await this.request(`/v1/runs/${encodeURIComponent(runId)}`, {
      method: "PATCH",
      body: JSON.stringify(input),
      headers: { "content-type": "application/json" },
    });
  }

  private async request<T = unknown>(path: string, init: RequestInit): Promise<T> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const response = await fetch(`${this.endpoint}${path}`, {
        ...init,
        signal: controller.signal,
      });
      if (!response.ok) {
        throw new Error(`looplog request failed: ${response.status} ${await response.text()}`);
      }
      return (await response.json()) as T;
    } finally {
      clearTimeout(timer);
    }
  }
}

export class LoopLogRun {
  constructor(
    private client: LoopLogClient,
    public readonly runId: string,
  ) {}

  append(lines: AppendLineInput[]): Promise<void> {
    return this.client.appendLines(this.runId, lines);
  }

  finish(input: FinishRunInput = {}): Promise<void> {
    return this.client.finishRun(this.runId, input);
  }
}
