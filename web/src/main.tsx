import React, { type FormEvent, useState } from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { Clapperboard } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { inputClass, primaryButton } from "@/components/common";
import { Layout } from "@/components/Layout";
import { BrowsePage } from "@/pages/BrowsePage";
import { FollowingPage } from "@/pages/FollowingPage";
import { SeriesPage } from "@/pages/SeriesPage";
import { StoreProvider, useStore } from "@/store";
import "./index.css";

function LoginPage() {
  const { login } = useStore();
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await login(password);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <main className="flex min-h-screen items-center justify-center bg-slate-950 p-4">
      <form onSubmit={submit} className="w-full max-w-sm space-y-4 rounded-xl border border-slate-700 bg-slate-900 p-6">
        <h1 className="flex items-center gap-2 text-lg font-semibold text-slate-100">
          <Clapperboard className="h-6 w-6 text-sky-400" aria-hidden />
          RÚV Sarpur
        </h1>
        <label className="block text-sm text-slate-200">
          Password
          <Input type="password" autoFocus autoComplete="current-password" className={`mt-1 ${inputClass}`} value={password} onChange={(e) => setPassword(e.target.value)} />
        </label>
        {error ? (
          <p role="alert" className="text-sm text-rose-200">
            {error}
          </p>
        ) : null}
        <Button type="submit" className={`w-full ${primaryButton}`} disabled={busy || !password}>
          Log in
        </Button>
      </form>
    </main>
  );
}

function App() {
  const { session } = useStore();
  if (!session) return <main className="min-h-screen bg-slate-950" aria-busy="true" />;
  if (session.required && !session.authenticated) return <LoginPage />;
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route path="/" element={<BrowsePage />} />
        <Route path="/title/:sid" element={<SeriesPage />} />
        <Route path="/following" element={<FollowingPage />} />
        <Route path="*" element={<BrowsePage />} />
      </Route>
    </Routes>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <StoreProvider>
        <App />
      </StoreProvider>
    </BrowserRouter>
  </React.StrictMode>,
);
