// ljos seat hook for opencode V2, at ~/.config/opencode/plugins/ljos.ts.
// Written by `ljos onboard --harness opencode-v2`; {ljos} is filled in there.
//
// opencode V2 does not run a V1 plugin, which returns hooks by string key.
// This one registers the same hooks on their V2 domains in setup(ctx):
// the seat's memories on each prompt, argv law on each bash call, and the
// session id in every shell the agent opens.
//
// A gate that cannot answer refuses: a missing binary, a timeout, a
// non-zero exit or bad JSON stops the bash call rather than letting it run.
import { Plugin } from "@opencode/plugin"

const LJOS = process.env.LJOS_BIN ?? "{ljos}"
const TIMEOUT_MS = 20_000

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

function text(parts: any[] | undefined): string {
  return (parts ?? [])
    .filter((p: any) => p?.type === "text" && !p.synthetic)
    .map((p: any) => p.text)
    .join("\n")
    .trim()
}

export default Plugin.define({
  id: "ljos.seat",
  async setup(ctx) {
    const cwd = ctx.location?.directory ?? process.cwd()
    // The context the last prompt raised, per session; the context hook
    // hands it to every model request of that turn.
    const held = new Map<string, string>()

    await ctx.shell.hook("create.before", (event: any) => {
      const id = event.sessionID ?? event.session?.id
      if (id) event.env.OPENCODE_SESSION_ID = id
    })

    await ctx.session.hook("prompt", async (event: any) => {
      const id = event.sessionID ?? event.session?.id
      const prompt = text(event.parts ?? event.message?.parts)
      if (!id || !prompt) return
      const a = await hook({ session_id: id, hook_event_name: "UserPromptSubmit", prompt, cwd })
      const add = a.ok ? a.out?.additionalContext : undefined
      if (add) held.set(id, add)
      else held.delete(id)
    })

    await ctx.session.hook("context", (event: any) => {
      const id = event.sessionID ?? event.session?.id
      const add = id ? held.get(id) : undefined
      if (add) event.system.push({ type: "text", text: `<ljos-seat-memory>\n${add}\n</ljos-seat-memory>` })
    })

    // A throw here reaches the model as the tool's error and the command
    // does not run. An ask stops the command and tells the agent to ask.
    await ctx.tool.hook("execute.before", async (event: any) => {
      const command = (event.input as { command?: unknown })?.command
      if (event.tool !== "bash" || typeof command !== "string") return
      const a = await hook({
        session_id: event.sessionID ?? event.session?.id,
        hook_event_name: "PreToolUse",
        tool_name: "Bash",
        tool_input: { command },
        cwd: (event.input as { workdir?: string })?.workdir ?? cwd,
      })
      if (!a.ok) throw new Error(`ljos: refused, the gate did not answer: ${a.why}`)
      const verdict = a.out?.permissionDecision
      if (verdict === "deny") throw new Error(`ljos: denied: ${a.out.permissionDecisionReason}`)
      if (verdict === "ask")
        throw new Error(
          `ljos: needs the person's approval: ${a.out.permissionDecisionReason}. ` +
            `Ask them; do not retry until they say yes.`,
        )
    })

    const stop = new AbortController()
    void (async () => {
      for await (const event of ctx.event.subscribe({ signal: stop.signal }) as any) {
        if (event?.type !== "session.deleted") continue
        const id = event.properties?.info?.id
        held.delete(id)
        await hook({ session_id: id, hook_event_name: "SessionEnd", cwd })
      }
    })()
    return () => stop.abort()
  },
})
