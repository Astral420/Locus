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
import { BookOpen, CheckCircle2, ChevronRight, FileText, LoaderCircle, Plus, Search, Send, ShieldCheck, Square, Upload } from "lucide-react";

const scopeLabels: Record<KnowledgeScope, string> = {
  this_meeting: "This meeting",
  all_meetings: "All meetings",
  documents_only: "Documents only",
  everything: "Everything",
};

function citationLabel(citation: KnowledgeMessageDTO["citations"][number]): string {
  if (citation.page_number) return citation.source_title + " · page " + citation.page_number;
  if (citation.start_seconds !== null) {
    const minutes = Math.floor(citation.start_seconds / 60);
    const seconds = Math.floor(citation.start_seconds % 60).toString().padStart(2, "0");
    return citation.source_title + " · " + minutes + ":" + seconds;
  }
  return citation.source_title;
}

export const KnowledgeContent: React.FC = () => {
  const queryClient = useQueryClient();
  const [selectedThreadId, setSelectedThreadId] = useState("th-1");
  const [scope, setScope] = useState<KnowledgeScope>("all_meetings");
  const [inputQuery, setInputQuery] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<KnowledgeSearchResultDTO[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [localMessages, setLocalMessages] = useState<KnowledgeMessageDTO[]>([]);
  const [streamingId, setStreamingId] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

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

  useEffect(() => {
    if (threads?.[0] && !threads.some((thread) => thread.id === selectedThreadId)) {
      setSelectedThreadId(threads[0].id);
      setScope(threads[0].scope);
    }
  }, [selectedThreadId, threads]);
  useEffect(() => () => {
    if (timer.current !== null) window.clearInterval(timer.current);
  }, []);

  const newThread = async (nextScope: KnowledgeScope) => {
    const meetingId = nextScope === "this_meeting" ? selectedThread?.meeting_id : null;
    if (nextScope === "this_meeting" && !meetingId) {
      setError("Choose “Ask about this meeting” from a meeting detail view to use this scope.");
      return;
    }
    const thread = await createKnowledgeThread(nextScope, meetingId);
    setScope(nextScope);
    setSelectedThreadId(thread.id);
    setLocalMessages([]);
    await queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
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
      <div className="flex-1 grid grid-cols-1 md:grid-cols-12 overflow-hidden">
        <aside className="md:col-span-4 xl:col-span-3 border-r border-border/80 bg-surface/30 p-4 flex flex-col min-h-0 overflow-y-auto gap-6" aria-label="Knowledge Base sidebar">
          <section>
            <div className="flex items-center justify-between mb-2"><span className="text-[11px] font-semibold text-ink-muted uppercase tracking-wider">Conversations</span><button type="button" onClick={() => void newThread(scope)} className="p-1 rounded text-primary hover:bg-surface focus-visible:ring-2 focus-visible:ring-accent" title="New conversation" aria-label="Start a new Knowledge conversation"><Plus className="w-4 h-4" /></button></div>
            <div className="space-y-1">{(threads || []).map((thread) => <button key={thread.id} type="button" onClick={() => { setSelectedThreadId(thread.id); setScope(thread.scope); setLocalMessages([]); }} className={"w-full text-left p-2.5 rounded-lg text-xs " + (activeThreadId === thread.id ? "bg-green-tint text-green-text font-semibold border border-primary/20" : "text-ink hover:bg-surface border border-transparent")}><p className="truncate">{thread.title}</p><span className="text-[10px] text-ink-muted font-normal mt-0.5 block">{scopeLabels[thread.scope]} · {thread.message_count} messages</span></button>)}{!threads?.length && <EmptyState icon={<BookOpen className="w-5 h-5 text-primary" />} title="No conversations" description="Start a cited Knowledge inquiry." />}</div>
          </section>

          <section>
            <div className="flex items-center justify-between mb-2"><span className="text-[11px] font-semibold text-ink-muted uppercase tracking-wider">Documents ({documents?.length || 0})</span><label className="p-1 rounded text-primary hover:bg-surface cursor-pointer focus-within:ring-2 focus-within:ring-accent" title="Upload document" aria-label="Upload document"><Upload className="w-3.5 h-3.5" /><input type="file" accept=".pdf,.md,.markdown,.txt,text/plain,text/markdown,application/pdf" className="sr-only" onChange={(event) => void upload(event)} /></label></div>
            <div className="space-y-1.5">{(documents || []).map((document) => <div key={document.id} className="p-2 rounded bg-surface-elevated border border-border/80 text-xs flex items-center justify-between gap-2"><div className="flex items-center gap-2 truncate"><FileText className="w-3.5 h-3.5 text-primary shrink-0" /><span className="truncate font-medium text-ink">{document.filename}</span></div><Badge variant={document.status === "error" ? "red" : document.status === "indexing" ? "amber" : "green"}>{document.status}</Badge></div>)}</div>
          </section>

          <form onSubmit={search} className="space-y-2"><label htmlFor="knowledge-search" className="text-[11px] font-semibold text-ink-muted uppercase tracking-wider">Semantic search</label><div className="flex gap-1.5"><input id="knowledge-search" value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder="Search by meaning…" className="min-w-0 flex-1 h-8 px-2.5 rounded border border-border bg-surface-elevated text-xs text-ink focus-visible:ring-2 focus-visible:ring-accent" /><Button variant="secondary" size="sm" type="submit" aria-label="Run semantic search"><Search className="w-3.5 h-3.5" /></Button></div>{searchResults.length > 0 && <div className="space-y-1.5" aria-label="Semantic search results">{searchResults.map((result) => <button key={result.chunk_id} type="button" className="w-full text-left p-2 rounded bg-surface-elevated border border-border text-[11px] hover:border-primary"><span className="block font-semibold text-primary">{result.source_title}</span><span className="line-clamp-2 text-ink-muted">{result.text}</span></button>)}</div>}</form>
          {error && <p role="alert" className="text-[11px] text-status-error">{error}</p>}
          <div className="mt-auto p-2.5 rounded bg-surface-sunken border border-border flex items-center justify-between text-xs"><span className="text-ink-muted font-medium">Vector index</span><span className={"flex items-center gap-1 font-semibold text-[11px] " + (indexStatus?.state === "blocked" ? "text-status-warning" : "text-green-text")}>{indexStatus?.state === "blocked" ? <LoaderCircle className="w-3.5 h-3.5" /> : <CheckCircle2 className="w-3.5 h-3.5 text-primary" />}{indexStatus?.state === "blocked" ? "Model needed" : indexStatus?.pending_sources ? "Indexing" : "Ready"}</span></div>
        </aside>

        <main className="md:col-span-8 xl:col-span-9 flex flex-col min-h-0 bg-surface-elevated">
          <div className="min-h-12 border-b border-border/80 px-6 py-2 flex flex-wrap items-center justify-between gap-3 bg-surface/20"><label className="flex items-center gap-2 text-xs"><span className="font-semibold text-ink-muted text-[11px] uppercase tracking-wider">Scope</span><select value={scope} onChange={(event) => void newThread(event.target.value as KnowledgeScope)} className="h-8 px-2 rounded border border-border bg-surface-elevated text-xs text-ink focus-visible:ring-2 focus-visible:ring-accent">{Object.entries(scopeLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label className="flex items-center gap-2 text-xs"><span className="font-semibold text-ink-muted text-[11px] uppercase tracking-wider">Destination</span><select aria-label="Chat provider destination" className="h-8 px-2 rounded border border-border bg-surface-elevated text-xs text-ink"><option>Local · llama-server</option><option>Ollama</option><option>OpenAI</option><option>Anthropic</option><option>Gemini</option></select><ShieldCheck className="w-4 h-4 text-primary" aria-label="Local destination" /></label></div>
          <div className="flex-1 p-6 overflow-y-auto space-y-4" aria-live="polite">
            {messages.length === 0 ? <EmptyState icon={<BookOpen className="w-8 h-8 text-primary" />} title="Start a Knowledge inquiry" description="Ask about decisions, concepts, or deadlines in the selected scope." actionLabel="Summarize recent decisions" onAction={() => setInputQuery("Summarize the key decisions from the selected sources.")} /> : messages.map((message) => <div key={message.id} className={"p-4 rounded-lg max-w-3xl text-xs sm:text-sm leading-relaxed " + (message.role === "user" ? "ml-auto bg-primary text-white font-medium" : "mr-auto bg-surface border border-border text-ink")}><p className="whitespace-pre-wrap">{message.content || (message.state === "pending" ? "Thinking…" : "")}</p>{message.state === "canceled" && <p className="mt-2 text-[11px] text-status-warning">[Incomplete Response]</p>}{message.citations.length > 0 && <div className="mt-3 pt-2.5 border-t border-border/40 space-y-1"><span className="font-semibold text-[11px] uppercase tracking-wider text-ink-muted block">Sources</span>{message.citations.map((citation) => <button key={citation.id} type="button" className="flex items-center gap-1.5 text-[11px] text-primary hover:underline" title="Citation location"><ChevronRight className="w-3 h-3" />{citationLabel(citation)}</button>)}</div>}</div>)}
          </div>
          <div className="p-4 border-t border-border bg-surface/30"><form onSubmit={send} className="flex items-center gap-3 max-w-3xl mx-auto"><input aria-label="Knowledge question" type="text" value={inputQuery} onChange={(event) => setInputQuery(event.target.value)} placeholder="Ask a question about your local sources…" className="flex-1 h-10 px-3.5 rounded-lg border border-border bg-surface-elevated text-xs sm:text-sm text-ink focus-visible:ring-2 focus-visible:ring-accent" />{streamingId ? <Button variant="secondary" size="md" type="button" onClick={cancel}><Square className="w-3.5 h-3.5 mr-1.5" />Stop</Button> : <Button variant="primary" size="md" type="submit" disabled={!inputQuery.trim()}><Send className="w-4 h-4 mr-1.5" />Send</Button>}</form><p className="max-w-3xl mx-auto mt-2 text-[10px] text-ink-muted">Responses are grounded in retrieved sources. The selected destination is shown before dispatch.</p></div>
        </main>
      </div>
    </div>
  );
};

export const KnowledgeView: React.FC = () => <ErrorBoundary fallbackTitle="Unable to load Knowledge Base"><KnowledgeContent /></ErrorBoundary>;
