// ljos seat hook for opencode, at ~/.config/opencode/plugins/ljos.ts.
// Written by `ljos onboard --harness opencode`; {ljos} is filled in there.
//
// The seat's memories on each user prompt, argv law on each bash call, and
// the session id in every shell the agent opens. Each payload names the
// working directory. A bash call waits on the gate: a missing binary, a
// timeout, a non-zero exit or bad JSON refuses it rather than letting the
// command run. The prompt and session hooks have no opinion when it fails.
import type { Plugin } from "@opencode-ai/plugin"

const LJOS = process.env.LJOS_BIN ?? "{ljos}"
const TIMEOUT_MS = 20_000

// The context the last prompt raised, per session; the system transform
// hands it to every model call of that turn.
const held = new Map<string, string>()
// Sessions that have heard the start line. opencode has no session-start
// hook a plugin can answer into, so the first prompt of a session carries it.
const started = new Set<string>()

type Answer = { ok: true; out: any | undefined } | { ok: false; why: string }

async function hook(payload: Record<string, unknown>): Promise<Answer> {
  let proc
  try {
    proc = Bun.spawn([LJOS, "hook"], {
      stdin: new Blob([JSON.stringify(payload)]),
      stdout: "pipe",
      stderr: "pipe",
    })
  } catch (e) {
    return { ok: false, why: `${LJOS} did not start: ${e}` }
  }
  const timer = setTimeout(() => proc.kill(), TIMEOUT_MS)
  try {
    const text = (await new Response(proc.stdout).text()).trim()
    const code = await proc.exited
    if (code !== 0) {
      const err = (await new Response(proc.stderr).text()).trim()
      return { ok: false, why: `ljos hook exited ${code}${err ? `: ${err}` : ""}` }
    }
    if (!text) return { ok: true, out: undefined }
    return { ok: true, out: JSON.parse(text).hookSpecificOutput }
  } catch (e) {
    return { ok: false, why: `ljos hook gave no answer: ${e}` }
  } finally {
    clearTimeout(timer)
  }
}

export const LjosSeat: Plugin = async ({ directory }) => ({
  // A shell the agent opens carries its session, so `ljos` there holds
  // under the conversation rather than under the shell's process.
  "shell.env": async (input, output) => {
    if (input.sessionID) output.env.OPENCODE_SESSION_ID = input.sessionID
  },

  "chat.message": async (input, output) => {
    const prompt = output.parts
      .filter((p: any) => p.type === "text" && !p.synthetic)
      .map((p: any) => p.text)
      .join("\n")
      .trim()
    if (!prompt) return
    const a = await hook({
      session_id: input.sessionID,
      hook_event_name: "UserPromptSubmit",
      prompt,
      cwd: directory,
    })
    let ctx: string | undefined = a.ok ? a.out?.additionalContext : undefined
    if (!started.has(input.sessionID)) {
      started.add(input.sessionID)
      const s = await hook({ session_id: input.sessionID, hook_event_name: "SessionStart", cwd: directory })
      const line = s.ok ? s.out?.additionalContext : undefined
      if (line) ctx = ctx ? `${line}\n${ctx}` : line
    }
    if (ctx) held.set(input.sessionID, ctx)
    else held.delete(input.sessionID)
  },

  "experimental.chat.system.transform": async (input, output) => {
    const ctx = input.sessionID ? held.get(input.sessionID) : undefined
    if (ctx) output.system.push(`<ljos-seat-memory>\n${ctx}\n</ljos-seat-memory>`)
  },

  // A throw here reaches the model as the tool's error and the command
  // does not run. opencode cannot raise its approval prompt from a plugin,
  // so an ask stops the command and tells the agent to ask the person.
  "tool.execute.before": async (input, output) => {
    if (input.tool !== "bash" || typeof output.args?.command !== "string") return
    const a = await hook({
      session_id: input.sessionID,
      hook_event_name: "PreToolUse",
      tool_name: "Bash",
      tool_input: { command: output.args.command },
      cwd: output.args.workdir ?? directory,
    })
    if (!a.ok) throw new Error(`ljos: refused, the gate did not answer: ${a.why}`)
    const out = a.out
    const verdict = out?.permissionDecision
    if (verdict === "deny") throw new Error(`ljos: denied: ${out.permissionDecisionReason}`)
    if (verdict === "ask")
      throw new Error(
        `ljos: needs the person's approval: ${out.permissionDecisionReason}. ` +
          `Ask them; do not retry until they say yes.`,
      )
  },

  event: async ({ event }) => {
    if (event.type === "session.deleted") {
      const id = (event as any).properties?.info?.id
      held.delete(id)
      started.delete(id)
      await hook({ session_id: id, hook_event_name: "SessionEnd", cwd: directory })
    }
  },
})
