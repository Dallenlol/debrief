import { useEffect, useState } from "react";
import { createHashRouter, Navigate, RouterProvider } from "react-router-dom";
import { Shell } from "@/components/Shell";
import { Spinner } from "@/components/ui";
import { cmd } from "@/lib/ipc";
import { installTheme } from "@/lib/theme";
import { AskPage } from "@/routes/Ask";
import { Library } from "@/routes/Library";
import { MeetingPage } from "@/routes/Meeting";
import { Onboarding } from "@/routes/Onboarding";
import { SettingsPage } from "@/routes/Settings";
import { Popup } from "@/windows/Popup";
import { RecordBar } from "@/windows/RecordBar";

installTheme();

const windowKind = new URLSearchParams(window.location.search).get("window");

export default function App() {
  if (windowKind === "popup" || windowKind === "recordbar") {
    document.body.classList.add("transparent");
    return windowKind === "popup" ? <Popup /> : <RecordBar />;
  }
  return <MainApp />;
}

function MainApp() {
  const [onboarded, setOnboarded] = useState<boolean | null>(null);
  useEffect(() => {
    cmd.getSettings().then((s) => setOnboarded(s.onboarded), () => setOnboarded(true));
  }, []);
  if (onboarded === null) return <div className="grid h-full place-items-center"><Spinner /></div>;

  const router = createHashRouter([
    { path: "/onboarding", element: <Onboarding /> },
    {
      path: "/",
      element: onboarded ? <Shell /> : <Navigate to="/onboarding" replace />,
      children: [
        { index: true, element: <Library /> },
        { path: "meeting/:id", element: <MeetingPage /> },
        { path: "ask", element: <AskPage /> },
        { path: "settings", element: <SettingsPage /> },
      ],
    },
  ]);
  return <RouterProvider router={router} />;
}
