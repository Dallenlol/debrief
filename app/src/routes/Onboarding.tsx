import { ArrowRight, Check, Download } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button, Card, Meter, Spinner } from "@/components/ui";
import { fmtBytes } from "@/lib/format";
import { cmd, subscribe, type Hardware, type ModelRow, type Settings, type Tier } from "@/lib/ipc";

const TIER_COPY: Record<Tier, string> = {
  cpu_low: "Light models for CPU-only machines. Captions may lag a few seconds.",
  cpu_high: "Bigger models on CPU. Good transcripts; summaries take a bit longer.",
  gpu: "GPU acceleration. Best transcripts and a strong local assistant.",
};

export function Onboarding() {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [levels, setLevels] = useState<[number, number]>([-100, -100]);
  const [hw, setHw] = useState<{ hardware: Hardware; tier: Tier; effective_tier: Tier } | null>(null);
  const [models, setModels] = useState<ModelRow[]>([]);
  const [progress, setProgress] = useState<Record<string, number>>({});

  useEffect(() => {
    void cmd.getSettings().then(setSettings);
    void cmd.probeHardware().then(setHw);
    void cmd.listModels().then(setModels);
    return subscribe("model_progress", (p) => {
      setProgress((m) => ({ ...m, [p.id]: p.total ? p.done / p.total : 0 }));
      if (p.status !== "downloading") void cmd.listModels().then(setModels);
    });
  }, []);

  useEffect(() => {
    if (step !== 0) return;
    let stop = false;
    const loop = async () => {
      while (!stop) {
        try {
          setLevels(await cmd.sampleLevels(null, null));
        } catch {
          /* ignore */
        }
      }
    };
    void loop();
    return () => {
      stop = true;
    };
  }, [step]);

  const finish = async () => {
    if (settings) await cmd.setSettings({ ...settings, onboarded: true });
    navigate("/", { replace: true });
  };

  const needed = models.filter((m) => m.roles.length > 0);
  const allPresent = needed.length > 0 && needed.every((m) => m.present);
  // Small models that make recording useful right away; the big ones can finish in the background.
  const isEssential = (m: ModelRow) => m.roles.some((r) => r === "live" || r === "speakers" || r === "search");
  const essentials = needed.filter(isEssential);
  const later = needed.filter((m) => !isEssential(m));
  const essentialsPresent = essentials.length > 0 && essentials.every((m) => m.present);
  const bytes = (xs: ModelRow[]) => xs.filter((m) => !m.present).reduce((n, m) => n + m.spec.size_bytes, 0);
  const startAll = (xs: ModelRow[]) => xs.filter((m) => !m.present && !m.downloading).forEach((m) => void cmd.downloadModel(m.spec.id).catch(() => {}));
  const ROLE_LABEL: Record<string, string> = { live: "live captions", quality: "final transcript", llm: "summaries & chat", speakers: "who spoke", search: "semantic search" };

  return (
    <div className="grain flex h-full items-center justify-center p-8">
      <div className="relative z-10 w-full max-w-xl">
        <div className="mb-8 flex items-center gap-3 text-[12px] font-medium text-ink-3">
          {["Audio", "Hardware", "Models"].map((label, i) => (
            <span key={label} className="flex items-center gap-3">
              <span className={i === step ? "text-ink" : i < step ? "text-moss" : ""}>{i < step ? <Check size={12} className="mr-1 inline" /> : null}{label}</span>
              {i < 2 && <span className="h-px w-8 bg-line-2" />}
            </span>
          ))}
        </div>

        {step === 0 && (
          <div className="rise">
            <h1 className="font-serif text-[40px] leading-none tracking-tight">Hark is listening for you, not to you.</h1>
            <p className="mt-3 text-[15px] text-ink-2">Recordings, transcripts and AI all stay on this computer. Let's check your microphone and system audio.</p>
            <Card className="mt-6 p-5">
              <Meter db={levels[0]} label="Mic" />
              <Meter db={levels[1]} label="System" className="mt-3" />
              <p className="mt-3 text-[12px] text-ink-3">
                Say something, and play any sound. Both bars should move. On macOS you may be asked to allow microphone and audio recording.
              </p>
            </Card>
            <Button variant="primary" size="lg" className="mt-6" onClick={() => setStep(1)}>
              Continue <ArrowRight size={16} />
            </Button>
          </div>
        )}

        {step === 1 && hw && (
          <div className="rise">
            <h1 className="font-serif text-[40px] leading-none tracking-tight">Sized for this machine.</h1>
            <p className="mt-3 text-[15px] text-ink-2">
              {hw.hardware.cpu_name || "CPU"}, {Math.round(hw.hardware.ram_gb)} GB RAM
              {hw.hardware.gpu ? `, ${hw.hardware.gpu.name} with ${Math.round(hw.hardware.gpu.vram_gb)} GB` : ", no supported GPU"}.
            </p>
            <div className="mt-6 grid gap-2">
              {(["gpu", "cpu_high", "cpu_low"] as Tier[]).map((t) => {
                const selected = (settings?.tier_override ?? hw.tier) === t;
                return (
                  <button
                    key={t}
                    onClick={() => settings && setSettings({ ...settings, tier_override: t === hw.tier ? null : t })}
                    className={`focus-ring rounded-lg border p-4 text-left transition-colors ${selected ? "border-ink bg-canvas shadow-card" : "border-line hover:bg-canvas-2"}`}
                  >
                    <div className="flex items-center gap-2 text-[14px] font-semibold">
                      {t === "gpu" ? "GPU" : t === "cpu_high" ? "CPU, roomy" : "CPU, light"}
                      {t === hw.tier && <span className="rounded bg-moss-soft px-1.5 text-[10px] font-semibold tracking-wide text-moss uppercase">Recommended</span>}
                    </div>
                    <div className="text-[13px] text-ink-2">{TIER_COPY[t]}</div>
                  </button>
                );
              })}
            </div>
            <div className="mt-6 flex gap-2">
              <Button variant="ghost" size="lg" onClick={() => setStep(0)}>Back</Button>
              <Button
                variant="primary"
                size="lg"
                onClick={async () => {
                  if (settings) await cmd.setSettings(settings);
                  setModels(await cmd.listModels());
                  setStep(2);
                }}
              >
                Continue <ArrowRight size={16} />
              </Button>
            </div>
          </div>
        )}

        {step === 2 && (
          <div className="rise">
            <h1 className="font-serif text-[40px] leading-none tracking-tight">Download the models.</h1>
            <p className="mt-3 text-[15px] text-ink-2">
              One-time download from Hugging Face, verified by checksum. Start with the essentials ({fmtBytes(bytes(essentials))}) and record right away; the bigger models ({fmtBytes(bytes(later))}) finish in the background and Hark catches up on clean-up and summaries by itself.
            </p>
            <Card className="mt-6 divide-y divide-line px-5">
              {needed.length === 0 && <div className="py-6"><Spinner /></div>}
              {[...essentials, ...later].map((m) => (
                <div key={m.spec.id} className="flex items-center gap-4 py-3">
                  <div className="min-w-0 flex-1">
                    <div className="text-[14px] font-medium">{m.spec.name} <span className="text-ink-3">{fmtBytes(m.spec.size_bytes)}</span></div>
                    <div className="text-[12px] text-ink-3">{isEssential(m) ? "Essential: " : "Later: "}{m.roles.map((r) => ROLE_LABEL[r] ?? r).join(", ")}</div>
                    {m.downloading && (
                      <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-line">
                        <div className="h-full bg-ink transition-[width]" style={{ width: `${Math.round((progress[m.spec.id] ?? 0) * 100)}%` }} />
                      </div>
                    )}
                  </div>
                  {m.present ? (
                    <span className="inline-flex items-center gap-1 text-[12px] font-medium text-moss"><Check size={14} /> Ready</span>
                  ) : m.downloading ? (
                    <span className="font-mono text-[12px] text-ink-2">{Math.round((progress[m.spec.id] ?? 0) * 100)}%</span>
                  ) : (
                    <Button variant="outline" size="sm" onClick={() => { void cmd.downloadModel(m.spec.id).catch(() => {}); void cmd.listModels().then(setModels); }}>
                      <Download size={14} /> Download
                    </Button>
                  )}
                </div>
              ))}
            </Card>
            <div className="mt-6 flex gap-2">
              <Button variant="ghost" size="lg" onClick={() => setStep(1)}>Back</Button>
              {!allPresent && !needed.some((m) => m.downloading) && (
                <Button variant="outline" size="lg" onClick={() => startAll(needed)}>
                  Download all
                </Button>
              )}
              {!essentialsPresent && !essentials.some((m) => m.downloading) ? (
                <Button variant="primary" size="lg" onClick={() => startAll(essentials)}>
                  <Download size={16} /> Start with essentials
                </Button>
              ) : (
                <Button
                  variant="primary"
                  size="lg"
                  onClick={() => {
                    if (essentialsPresent) startAll(later);
                    void finish();
                  }}
                >
                  {allPresent ? "Open Hark" : essentialsPresent ? "Open Hark, finish the rest in the background" : "Skip for now"} <ArrowRight size={16} />
                </Button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
