import { AppendLineInput, LoopLogClient, LoopLogMeta, LoopLogRun } from "../../sdk/ts/looplog";

export interface WechatMiniProgramContext {
  project_path: string;
  appid?: string;
  page?: string;
  query?: LoopLogMeta;
  scene?: string;
  compile_mode?: string;
  tool?: string;
  tool_version?: string;
  base_lib_version?: string;
  platform?: string;
  device?: string;
  network?: string;
  session?: string;
  trace_id?: string;
}

export class WechatMiniProgramAdapter {
  constructor(private client = new LoopLogClient()) {}

  async startConsoleRun(context: WechatMiniProgramContext): Promise<LoopLogRun> {
    return this.client.startRun({
      tag: "wx-console",
      source: context.tool ?? "wechat-devtools",
      kind: "wechat_miniprogram",
      cwd: context.project_path,
      meta: context,
    });
  }

  consoleLine(level: string, text: string, meta: LoopLogMeta = {}): AppendLineInput {
    return {
      stream: "console",
      level,
      event: "console",
      text,
      meta,
    };
  }

  networkLine(text: string, meta: LoopLogMeta = {}): AppendLineInput {
    return {
      stream: "network",
      level: "info",
      event: "network",
      text,
      meta,
    };
  }
}
