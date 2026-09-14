import { ArrowUp, Plus, Square, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { cmd, subscribe, type Chat, type ChatMessage } from "@/lib/ipc";
import { Markdown } from "./Markdown";
import { Button, Spinner } from "./ui";

interface ChatPanelProps {
  scopeKind: "meeting" | "all";
  scopeId?: string | null;
  /** Called for a citation click; `meetingId` is null when unresolved. */
  onSeek: (ms: number, meetingId: string | null) => void;
  placeholder?: string;
  className?: string;
}

/** Chat over one meeting or the whole library. Streams the assistant reply. */
export function ChatPanel({ scopeKind, scopeId = null, onSeek, placeholder, className }: ChatPanelProps) {
  const [chats, setChats] = useState<Chat[]>([]);
  const [chatId, setChatId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [streaming, setStreaming] = useState<string | null>(null); // assistant message id
  const [error, setError] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);

  const loadChats = async () => {
    const list = await cmd.listChats(scopeKind, scopeId);
    setChats(list);
    if (!chatId && list[0]) setChatId(list[0].id);
    if (chatId && !list.some((c) => c.id === chatId)) setChatId(list[0]?.id ?? null);
  };

  useEffect(() => {
    void loadChats();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeKind, scopeId]);

  useEffect(() => {
    if (!chatId) {
      setMessages([]);
      return;
    }
    void cmd.chatMessages(chatId).then(setMessages);
  }, [chatId]);

  useEffect(() => {
    const a = subscribe("chat_token", (p) => {
      if (p.chat_id !== chatId) return;
      setMessages((ms) => ms.map((m) => (m.id === p.message_id ? { ...m, content: m.content + p.delta } : m)));
    });
    const b = subscribe("chat_done", (p) => {
      if (p.chat_id !== chatId) return;
      setStreaming(null);
      setError(p.error);
      setMessages((ms) => ms.map((m) => (m.id === p.message.id ? p.message : m)));
      void loadChats();
    });
    return () => {
      a();
      b();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId]);

  useEffect(() => {
    bottom.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, streaming]);

  const send = async () => {
    const text = draft.trim();
    if (!text || streaming) return;
    setError(null);
    let id = chatId;
    if (!id) {
      const c = await cmd.createChat(scopeKind, scopeId, undefined);
      id = c.id;
      setChatId(id);
      setChats((cs) => [c, ...cs]);
    }
    setDraft("");
    const userMsg: ChatMessage = { id: `tmp-${Date.now()}`, chat_id: id, role: "user", content: text, citations: [], created_at: new Date().toISOString() };
    setMessages((ms) => [...ms, userMsg]);
    try {
      const assistant = await cmd.sendChat(id, text);
      setMessages((ms) => [...ms, assistant]);
      setStreaming(assistant.id);
    } catch (e) {
      setError(String(e));
      setMessages((ms) => ms.filter((m) => m.id !== userMsg.id));
      setDraft(text);
    }
  };

  const newChat = async () => {
    const c = await cmd.createChat(scopeKind, scopeId, undefined);
    setChats((cs) => [c, ...cs]);
    setChatId(c.id);
  };

  const removeChat = async (id: string) => {
    await cmd.deleteChat(id);
    await loadChats();
    if (chatId === id) setChatId(null);
  };

  return (
    <div className={cn("flex min-h-0 flex-col", className)}>
      {chats.length > 0 && (
        <div className="mb-2 flex items-center gap-1 overflow-x-auto pb-1 text-[12px]">
          {chats.map((c) => (
            <span key={c.id} className={cn("group inline-flex shrink-0 items-center gap-1 rounded-full border px-2.5 py-0.5", c.id === chatId ? "border-ink bg-canvas" : "border-line text-ink-3 hover:text-ink")}>
              <button onClick={() => setChatId(c.id)} className="max-w-[160px] truncate">{c.title}</button>
              <button onClick={() => void removeChat(c.id)} className="text-ink-3 opacity-0 group-hover:opacity-100 hover:text-ember" title="Delete chat">
                <Trash2 size={11} />
              </button>
            </span>
          ))}
          <button onClick={() => void newChat()} className="inline-flex shrink-0 items-center gap-1 rounded-full border border-dashed border-line-2 px-2.5 py-0.5 text-ink-3 hover:text-ink" title="New chat">
            <Plus size={12} /> New
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
        {messages.length === 0 && (
          <div className="rise my-8 text-center text-[13px] text-ink-3">
            {placeholder ?? "Ask anything about this meeting. Answers cite timestamps you can click."}
          </div>
        )}
        {messages.map((m) => (
          <div key={m.id} className={cn("flex", m.role === "user" ? "justify-end" : "justify-start")}>
            <div className={cn("max-w-[85%] rounded-xl px-3.5 py-2", m.role === "user" ? "bg-ink text-canvas" : "border border-line bg-canvas shadow-card")}>
              {m.role === "user" ? (
                <p className="text-[14px] leading-6">{m.content}</p>
              ) : m.content === "" && streaming === m.id ? (
                <Spinner className="my-1" />
              ) : (
                <Markdown text={m.content} onSeek={(ms, label) => onSeek(ms, resolveMeeting(m, label))} />
              )}
            </div>
          </div>
        ))}
        {error && <div className="text-[12px] text-ember">{error}</div>}
        <div ref={bottom} />
      </div>

      <form
        className="mt-3 flex items-end gap-2 rounded-xl border border-line-2 bg-canvas p-1.5 pl-3 shadow-card focus-within:border-ink"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
          rows={1}
          placeholder="Ask Hark..."
          className="max-h-32 min-h-[28px] flex-1 resize-none bg-transparent py-1 text-[14px] outline-none placeholder:text-ink-3"
        />
        {streaming ? (
          <Button variant="soft" size="icon" type="button" onClick={() => chatId && void cmd.cancelChat(chatId)} title="Stop">
            <Square size={14} fill="currentColor" />
          </Button>
        ) : (
          <Button variant="primary" size="icon" type="submit" disabled={!draft.trim()} title="Send">
            <ArrowUp size={16} />
          </Button>
        )}
      </form>
    </div>
  );
}

function resolveMeeting(m: ChatMessage, label: string): string | null {
  const cites = Array.isArray(m.citations) ? (m.citations as { ms: number; meeting_id: string | null; label: string }[]) : [];
  return cites.find((c) => c.label === label)?.meeting_id ?? null;
}
