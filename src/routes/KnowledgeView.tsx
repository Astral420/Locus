import React, { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { Skeleton } from "../components/ui/Skeleton";
import { EmptyState } from "../components/ui/EmptyState";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import {
  createKnowledgeThread,
  getKnowledgeIndexStatus,
  listDocuments,
  listKnowledgeMessages,
  listKnowledgeThreads,
  searchKnowledge,
  sendKnowledgeMessage,
  uploadDocument,
  type DocumentDTO,
  type KnowledgeIndexStatusDTO,
  type KnowledgeMessageDTO,
  type KnowledgeScope,
  type KnowledgeSearchResultDTO,
  type KnowledgeThreadDTO,
} from "../lib/tauri";
import { BookOpen, CheckCircle2, ChevronDown, FileText, LoaderCircle, Search, Send, ShieldCheck, Square, Upload } from "lucide-react";
import { KNOWLEDGE_SCOPE_LABELS, openKnowledgeThread } from "../lib/knowledgeThreads";
import { useKnowledgeStore } from "../stores/knowledgeStore";

const scopeLabels = KNOWLEDGE_SCOPE_LABELS;

function citationLabel(citation: KnowledgeMessageDTO["citations"][number]): string {
  if (citation.page_number) return citation.source_title + " · page " + citation.page_number;
  if (citation.start_seconds !== null) {
    const minutes = Math.floor(citation.start_seconds / 60);
    const seconds = Math.floor(citation.start_seconds % 60).toString().padStart(2, "0");
    return citation.source_title + " · " + minutes + ":" + seconds;
  }
  return citation.source_title;
}

function messageTime(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/** Messaging-style bubble: user on the right (compact, neutral), assistant on the left (card with collapsible sources). */
const MessageBubble: React.FC<{ message: KnowledgeMessageDTO }> = ({ message }) => {
  const [showSources, setShowSources] = useState(true);
  const time = messageTime(message.created_at);

  if (message.role === "user") {
    return (
      <div className="flex justify-end" data-role="user">
        <div className="w-fit max-w-[75%] rounded-2xl rounded-br-md bg-surface-hover px-4 py-2.5 text-sm leading-relaxed text-ink">
          <p className="whitespace-pre-wrap break-words">{message.content}</p>
          {time && <span className="mt-1 block text-right text-[11px] text-ink-subtle">{time}</span>}
        </div>
      </div>
    );
  }

  const citations = message.citations;
  return (
    <div className="flex justify-start" data-role="assistant">
      <div className="w-fit max-w-[88%] rounded-2xl rounded-bl-md bg-surface px-4 py-3 text-sm leading-relaxed text-ink">
        <p className={"whitespace-pre-wrap break-words" + (message.content ? "" : " text-ink-muted animate-pulse")}>
          {message.content || (message.state === "pending" ? "Thinking…" : "")}
        </p>
        {message.state === "canceled" && <p className="mt-2 text-[11px] text-status-warning">[Incomplete Response]</p>}

        {citations.length > 0 && (
          <div className="mt-3 space-y-2">
            <button
              type="button"
              aria-expanded={showSources}
              onClick={() => setShowSources((v) => !v)}
              className="flex w-full items-center justify-between gap-6 rounded-xl bg-bg px-3 py-2 text-xs font-medium text-ink-muted hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              <span className="inline-flex items-center gap-2">
                <FileText className="h-3.5 w-3.5" aria-hidden="true" />
                Sources ({citations.length})
              </span>
              <ChevronDown className={"h-3.5 w-3.5 transition-transform " + (showSources ? "rotate-180" : "")} aria-hidden="true" />
            </button>
            {showSources && (
              <div className="flex flex-wrap gap-2">
                {citations.map((citation) => (
                  <button
                    key={citation.id}
                    type="button"
                    title="Citation location"
                    className="inline-flex max-w-full items-center gap-1.5 rounded-full bg-bg px-3 py-1 text-[11px] text-primary hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                  >
                    <span className="truncate">{citationLabel(citation)}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        {time && <span className="mt-2 block text-[11px] text-ink-subtle">{time}</span>}
      </div>
    </div>
  );
};

export const KnowledgeContent: React.FC = () => {
  const queryClient = useQueryClient();
  const selectedThreadId = useKnowledgeStore((s) => s.selectedThreadId);
  const setSelectedThreadId = useKnowledgeStore((s) => s.setSelectedThreadId);
  const [scope, setScope] = useState<KnowledgeScope>("all_meetings");
  const [inputQuery, setInputQuery] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<KnowledgeSearchResultDTO[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [localMessages, setLocalMessages] = useState<KnowledgeMessageDTO[]>([]);
  const [streamingId, setStreamingId] = useState<string | null>(null);
  const timer = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const { data: threads, isLoading: loadingThreads } = useQuery<KnowledgeThreadDTO[]>({
    queryKey: ["knowledgeThreads"],
    queryFn: listKnowledgeThreads,
  });
  const { data: documents, isLoading: loadingDocuments } = useQuery<DocumentDTO[]>({
    queryKey: ["documents"],
    queryFn: listDocuments,
  });
  const { data: indexStatus } = useQuery<KnowledgeIndexStatusDTO>({
    queryKey: ["knowledgeIndexStatus"],
    queryFn: getKnowledgeIndexStatus,
    refetchInterval: 3000,
  });
  const selectedThread = useMemo(
    () => threads?.find((thread) => thread.id === selectedThreadId) ?? threads?.[0],
    [selectedThreadId, threads]
  );
  const activeThreadId = selectedThread?.id ?? "";
  const { data: persistedMessages = [] } = useQuery<KnowledgeMessageDTO[]>({
    queryKey: ["knowledgeMessages", activeThreadId],
    queryFn: () => listKnowledgeMessages(activeThreadId),
    enabled: Boolean(activeThreadId),
  });
  const messages = persistedMessages.concat(localMessages.filter((message) => message.thread_id === activeThreadId));

  // The conversation list lives in the app sidebar; keep the fixed scope in step with whichever thread it opens.
  useEffect(() => {
    if (selectedThread) setScope(selectedThread.scope);
  }, [selectedThread?.id, selectedThread?.scope]);
  useEffect(() => {
    if (threads?.[0] && !threads.some((thread) => thread.id === selectedThreadId)) {
      setSelectedThreadId(threads[0].id);
      setScope(threads[0].scope);
    }
  }, [selectedThreadId, threads]);
  useEffect(() => () => {
    if (timer.current !== null) window.clearInterval(timer.current);
  }, []);
  // Chat-style: keep the newest message (and streaming text) in view.
  const lastMessageLength = messages[messages.length - 1]?.content.length ?? 0;
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages.length, lastMessageLength, activeThreadId]);

  const newThread = async (nextScope: KnowledgeScope) => {
    const meetingId = nextScope === "this_meeting" ? selectedThread?.meeting_id : null;
    if (nextScope === "this_meeting" && !meetingId) {
      setError("Choose “Ask about this meeting” from a meeting detail view to use this scope.");
      return;
    }
    try {
      // Reuses an existing empty conversation with this scope instead of piling up identical new ones.
      await openKnowledgeThread(queryClient, threads ?? [], nextScope, meetingId);
      setScope(nextScope);
      setLocalMessages([]);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "A Knowledge conversation could not be created.");
    }
  };

  const send = async (event: React.FormEvent) => {
    event.preventDefault();
    const query = inputQuery.trim();
    if (!query || streamingId) return;
    let threadId = activeThreadId;
    if (!threadId) {
      try {
        const thread = await createKnowledgeThread(
          scope,
          scope === "this_meeting" ? selectedThread?.meeting_id : null
        );
        threadId = thread.id;
        setSelectedThreadId(thread.id);
        await queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : "A Knowledge conversation could not be created.");
        return;
      }
    }
    setInputQuery("");
    setError(null);
    setLocalMessages((current) => current.concat({
      id: "pending-user-" + Date.now(),
      thread_id: threadId,
      ordinal: messages.length,
      role: "user",
      content: query,
      state: "complete",
      provider_identity: null,
      model_identity: null,
      created_at: new Date().toISOString(),
      citations: [],
    }));
    try {
      const answer = await sendKnowledgeMessage(threadId, query);
      void queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
      setLocalMessages((current) => current.concat({ ...answer, content: "" }));
      setStreamingId(answer.id);
      let offset = 0;
      timer.current = window.setInterval(() => {
        offset = Math.min(answer.content.length, offset + 4);
        setLocalMessages((current) => current.map((message) => message.id === answer.id ? { ...message, content: answer.content.slice(0, offset) } : message));
        if (offset >= answer.content.length && timer.current !== null) {
          window.clearInterval(timer.current);
          timer.current = null;
          setStreamingId(null);
          void queryClient.invalidateQueries({ queryKey: ["knowledgeMessages", threadId] });
        }
      }, 18);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The selected provider could not answer.");
    }
  };

  const cancel = () => {
    if (!streamingId) return;
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    setLocalMessages((current) => current.map((message) => message.id === streamingId ? { ...message, content: message.content + "\n\n[Incomplete Response]", state: "canceled" } : message));
    setStreamingId(null);
  };

  const upload = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setError(null);
    try {
      const result = await uploadDocument(file.name, file.type || "application/octet-stream", await file.arrayBuffer());
      await queryClient.invalidateQueries({ queryKey: ["documents"] });
      if (result.duplicate) setError("That document is already in the Knowledge Base; the existing copy was kept.");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Document upload failed.");
    }
  };

  const search = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!searchQuery.trim()) return;
    try {
      setSearchResults(await searchKnowledge(scope, searchQuery.trim(), selectedThread?.meeting_id));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Semantic search is unavailable.");
    }
  };

  if (loadingThreads || loadingDocuments) {
    return <div className="p-6 grid grid-cols-12 gap-6"><div className="col-span-4 space-y-3"><Skeleton className="h-9 w-full" /><Skeleton className="h-24 w-full" /></div><div className="col-span-8"><Skeleton className="h-64 w-full" /></div></div>;
  }

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-bg">
      <Header title="Knowledge Base" subtitle="Cited, fixed-scope chat over local meetings and uploaded documents" />
      <div className="flex-1 grid grid-cols-1 md:grid-cols-12 gap-4 px-6 pb-6 overflow-hidden">
        <aside className="md:col-span-4 xl:col-span-3 rounded-2xl bg-surface p-4 flex flex-col min-h-0 overflow-y-auto gap-6" aria-label="Knowledge sources">
          <section>
            <div className="flex items-center justify-between mb-2"><span className="text-[11px] font-semibold text-ink-muted uppercase tracking-wider">Documents ({documents?.length || 0})</span><label className="p-1.5 rounded-full text-primary hover:bg-surface-hover cursor-pointer focus-within:ring-2 focus-within:ring-accent" title="Upload document" aria-label="Upload document"><Upload className="w-3.5 h-3.5" /><input type="file" accept=".pdf,.md,.markdown,.txt,text/plain,text/markdown,application/pdf" className="sr-only" onChange={(event) => void upload(event)} /></label></div>
            <div className="space-y-1.5">{(documents || []).map((document) => <div key={document.id} className="p-2.5 rounded-lg bg-bg text-xs flex items-center justify-between gap-2"><div className="flex items-center gap-2 truncate"><FileText className="w-3.5 h-3.5 text-primary shrink-0" /><span className="truncate font-medium text-ink">{document.filename}</span></div><Badge variant={document.status === "error" ? "red" : document.status === "indexing" ? "amber" : "green"}>{document.status}</Badge></div>)}</div>
          </section>

          <form onSubmit={search} className="space-y-2"><label htmlFor="knowledge-search" className="text-[11px] font-semibold text-ink-muted uppercase tracking-wider">Semantic search</label><div className="flex gap-1.5"><input id="knowledge-search" value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder="Search by meaning…" className="min-w-0 flex-1 h-8 px-3 rounded-full bg-bg text-xs text-ink focus-visible:ring-2 focus-visible:ring-accent" /><Button variant="secondary" size="sm" type="submit" aria-label="Run semantic search"><Search className="w-3.5 h-3.5" /></Button></div>{searchResults.length > 0 && <div className="space-y-1.5" aria-label="Semantic search results">{searchResults.map((result) => <button key={result.chunk_id} type="button" className="w-full text-left p-2.5 rounded-lg bg-bg text-[11px] hover:bg-surface-hover"><span className="block font-semibold text-primary">{result.source_title}</span><span className="line-clamp-2 text-ink-muted">{result.text}</span></button>)}</div>}</form>
          {error && <p role="alert" className="text-[11px] text-status-error">{error}</p>}
          <div className="mt-auto p-2.5 rounded-xl bg-bg flex items-center justify-between text-xs"><span className="text-ink-muted font-medium">Vector index</span><span className={"flex items-center gap-1 font-semibold text-[11px] " + (indexStatus?.state === "blocked" ? "text-status-warning" : "text-green-text")}>{indexStatus?.state === "blocked" ? <LoaderCircle className="w-3.5 h-3.5" /> : <CheckCircle2 className="w-3.5 h-3.5 text-primary" />}{indexStatus?.state === "blocked" ? "Model needed" : indexStatus?.pending_sources ? "Indexing" : "Ready"}</span></div>
        </aside>

        <main className="md:col-span-8 xl:col-span-9 flex flex-col min-h-0">
          <div className="min-h-12 px-1 pt-1.5 pb-3 flex flex-wrap items-center justify-between gap-3">
            <label className="flex items-center gap-2 text-xs">
              <span className="font-semibold text-ink-muted text-[11px] uppercase tracking-wider">Scope</span><select value={scope} onChange={(event) => void newThread(event.target.value as KnowledgeScope)} className="h-8 pl-3 pr-2 rounded-full border border-border bg-surface-hover text-xs text-ink focus-visible:ring-2 focus-visible:ring-accent">{Object.entries(scopeLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label className="flex items-center gap-2 text-xs">
              <span className="font-semibold text-ink-muted text-[11px] uppercase tracking-wider">Destination</span>
              <select aria-label="Chat provider destination" className="h-8 pl-3 pr-2 rounded-full border border-border bg-surface-hover text-xs text-ink">
                <option>Local · llama-server</option>
                <option>Ollama</option><option>OpenAI</option>
                <option>Anthropic</option>
                <option>Gemini</option>
              </select>
            </label>
          </div>
          <div ref={scrollRef} className="flex-1 overflow-y-auto" aria-live="polite">
            <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 py-4 pr-1">
              {messages.length === 0 ? <EmptyState icon={<BookOpen className="w-8 h-8 text-primary" />} title="Start a Knowledge inquiry" description="Ask about decisions, concepts, or deadlines in the selected scope." actionLabel="Summarize recent decisions" onAction={() => setInputQuery("Summarize the key decisions from the selected sources.")} /> : messages.map((message) => <MessageBubble key={message.id} message={message} />)}
            </div>
          </div>
          <div className="pt-2"><form onSubmit={send} className="flex items-center gap-3 max-w-3xl mx-auto rounded-3xl border border-border bg-surface pl-5 pr-2.5 py-2"><input aria-label="Knowledge question" type="text" value={inputQuery} onChange={(event) => setInputQuery(event.target.value)} placeholder="Ask a question about your local sources…" className="flex-1 h-9 bg-transparent text-sm text-ink placeholder:text-ink-subtle focus-visible:outline-none" />{streamingId ? <Button variant="secondary" size="sm" type="button" onClick={cancel}><Square className="w-3.5 h-3.5" />Stop</Button> : <Button variant="primary" size="icon" type="submit" aria-label="Send" title="Send" disabled={!inputQuery.trim()}><Send className="w-4 h-4" /></Button>}</form><p className="max-w-3xl mx-auto mt-2 text-[10px] text-ink-muted">Responses are grounded in retrieved sources. The selected destination is shown before dispatch.</p></div>
        </main>
      </div>
    </div>
  );
};

export const KnowledgeView: React.FC = () => <ErrorBoundary fallbackTitle="Unable to load Knowledge Base"><KnowledgeContent /></ErrorBoundary>;
