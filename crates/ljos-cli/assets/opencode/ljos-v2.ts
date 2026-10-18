// ljos seat hook for opencode V2, at ~/.config/opencode/plugins/ljos.ts.
// Written by `ljos onboard --harness opencode-v2`; {ljos} is filled in there.
//
// opencode V2 does not run a V1 plugin, which returns hooks by string key.
// This one registers the same hooks on their V2 domains in setup(ctx):
// the seat's memories on each prompt, argv law on each bash call, the book
// on each edit and each git commit, and the session id in every shell the
// agent opens. The first prompt of a session also carries the start line.
//
// A gate that cannot answer refuses: a missing binary, a timeout, a
// non-zero exit or bad JSON stops the bash call rather than letting it run.
import { Plugin } from "@opencode/plugin"

const LJOS = process.env.LJOS_BIN ?? "{ljos}"
const TIMEOUT_MS = 20_000

type Answer = { ok: true; out: any | undefined } | { ok: false; why: string }

function seatTool(name: string): { tool: string; bash: boolean } | undefined {
  const n = (name ?? "").toLowerCase().replace(/-/g, "_")
  if (n === "bash" || n === "shell") return { tool: "Bash", bash: true }
  const edits: Record<string, string> = {
    edit: "Edit",
    write: "Write",
    multiedit: "MultiEdit",
    multi_edit: "MultiEdit",
    apply_patch: "apply_patch",
    applypatch: "apply_patch",
    strreplace: "Edit",
    str_replace: "Edit",
    write_to_file: "Write",
    replace_file_content: "Edit",
    edit_file: "Edit",
    notebookedit: "NotebookEdit",
    notebook_edit: "NotebookEdit",
  }
  const tool = edits[n]
  return tool ? { tool, bash: false } : undefined
}

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
    // Sessions that have heard the start line. The first prompt carries it.
    const started = new Set<string>()
    // A before-hook refusal still reaches execute.after on some builds.
    const blocked = new Map<string, string>()
    // The before hook sees the command. The after hook cites the commit
    // from it when the result does not carry the args.
    const commands = new Map<string, string>()
    const blockKey = (sessionID: string | undefined, tool: string) =>
      `${sessionID ?? ""}:${tool}`

    await ctx.shell.hook("create.before", (event: any) => {
      const id = event.sessionID ?? event.session?.id
      if (id) event.env.OPENCODE_SESSION_ID = id
    })

    await ctx.session.hook("prompt", async (event: any) => {
      const id = event.sessionID ?? event.session?.id
      const prompt = text(event.parts ?? event.message?.parts)
      if (!id || !prompt) return
      const a = await hook({ session_id: id, hook_event_name: "UserPromptSubmit", prompt, cwd })
      let add: string | undefined = a.ok ? a.out?.additionalContext : undefined
      if (!started.has(id)) {
        started.add(id)
        const s = await hook({ session_id: id, hook_event_name: "SessionStart", cwd })
        const line = s.ok ? s.out?.additionalContext : undefined
        if (line) add = add ? `${line}\n${add}` : line
      }
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
      const seat = seatTool(event.tool)
      if (!seat) return
      const input = event.input as { command?: unknown; workdir?: string } | undefined
      const command = input?.command
      const workdir = input?.workdir
      const key = blockKey(event.sessionID ?? event.session?.id, event.tool)
      if (typeof command === "string") commands.set(key, command)
      if (seat.bash && typeof command !== "string") return
      // cwd is the session directory. A relative workdir is resolved against
      // it, the way opencode's shell tool joins path.resolve(directory, workdir).
      const tool_input = seat.bash
        ? typeof workdir === "string" && workdir !== ""
          ? { command, workdir }
          : { command }
        : (event.input ?? {})
      const a = await hook({
        session_id: event.sessionID ?? event.session?.id,
        hook_event_name: "PreToolUse",
        tool_name: seat.tool,
        tool_input,
        cwd,
      })
      if (!seat.bash) return
      if (!a.ok) {
        blocked.set(key, "deny")
        throw new Error(`ljos: refused, the gate did not answer: ${a.why}`)
      }
      const verdict = a.out?.permissionDecision
      if (verdict === "deny" || verdict === "ask") blocked.set(key, verdict)
      if (verdict === "deny") throw new Error(`ljos: denied: ${a.out.permissionDecisionReason}`)
      if (verdict === "ask")
        throw new Error(
          `ljos: needs the person's approval: ${a.out.permissionDecisionReason}. ` +
            `Ask them; do not retry until they say yes.`,
        )
    })

    // The commit is cited here, after it exits. A zero exit still cites it
    // when the result also carries stderr.
    await ctx.tool.hook("execute.after", async (event: any) => {
      const seat = seatTool(event.tool)
      if (!seat) return
      const key = blockKey(event.sessionID ?? event.session?.id, event.tool)
      const verdict = blocked.get(key)
      if (verdict) blocked.delete(key)
      const args = event.args ?? event.input ?? {}
      let command = typeof args.command === "string" ? args.command : commands.get(key)
      commands.delete(key)
      if (seat.bash && typeof command !== "string") return
      const workdir = args.workdir
      const output = event.output ?? {}
      const meta = output.metadata ?? {}
      const exit = meta.exit ?? meta.exitCode ?? meta.exit_code
      const knownFail = exit !== undefined && exit !== null && String(exit) !== "0"
      const rawError = typeof output.error === "string" ? output.error : ""
      const error = verdict ? `ljos: ${verdict}` : knownFail ? rawError || `exit ${exit}` : ""
      const tool_response: Record<string, unknown> = {}
      if (exit !== undefined && exit !== null) tool_response.exit_code = exit
      if (error) tool_response.error = error
      const tool_input = seat.bash
        ? typeof workdir === "string" && workdir !== ""
          ? { command, workdir }
          : { command }
        : args
      await hook({
        session_id: event.sessionID ?? event.session?.id,
        hook_event_name: "PostToolUse",
        tool_name: seat.tool,
        tool_input,
        cwd,
        ...(verdict ? { permissionDecision: verdict } : {}),
        ...(Object.keys(tool_response).length ? { tool_response } : {}),
      })
    })

    const stop = new AbortController()
    void (async () => {
      for await (const event of ctx.event.subscribe({ signal: stop.signal }) as any) {
        if (event?.type === "session.idle") {
          const id = event.properties?.sessionID ?? event.properties?.info?.id
          await hook({ session_id: id, hook_event_name: "Stop", cwd })
          continue
        }
        if (event?.type !== "session.deleted") continue
        const id = event.properties?.info?.id
        held.delete(id)
        started.delete(id)
        await hook({ session_id: id, hook_event_name: "SessionEnd", cwd })
      }
    })()
    return () => stop.abort()
  },
})
