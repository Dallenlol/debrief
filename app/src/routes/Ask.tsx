import { useNavigate } from "react-router-dom";
import { ChatPanel } from "@/components/ChatPanel";

/** "Ask Hark" across every meeting. Citations open the meeting at that time. */
export function AskPage() {
  const navigate = useNavigate();
  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col px-8 py-8">
      <header className="mb-4">
        <h1 className="font-serif text-[34px] leading-none tracking-tight">Ask Hark</h1>
        <p className="mt-1.5 text-[13px] text-ink-3">Questions across all your meetings, answered by the model running on this computer.</p>
      </header>
      <ChatPanel
        scopeKind="all"
        onSeek={(ms, meetingId) => {
          if (meetingId) navigate(`/meeting/${meetingId}?t=${ms}`);
        }}
        placeholder="Try: What did we decide about pricing? Who owns the Stripe integration?"
        className="min-h-0 flex-1"
      />
    </div>
  );
}
