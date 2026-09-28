CREATE TABLE IF NOT EXISTS meetings (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  title_origin TEXT NOT NULL CHECK (title_origin IN ('placeholder','automatic','user')),
  title_revision INTEGER NOT NULL DEFAULT 0 CHECK (title_revision >= 0),
  recorded_at TEXT NOT NULL,
  timezone TEXT NOT NULL,
  duration_seconds REAL NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
  requested_type TEXT NOT NULL DEFAULT 'auto' CHECK (requested_type IN ('auto','meeting','lecture')),
  detected_type TEXT CHECK (detected_type IS NULL OR detected_type IN ('meeting','lecture','generic')),
  language_selected TEXT,
  language_detected TEXT,
  capture_sources TEXT NOT NULL DEFAULT '[]',
  single_person_mic INTEGER NOT NULL DEFAULT 0 CHECK (single_person_mic IN (0,1)),
  lifecycle TEXT NOT NULL DEFAULT 'recording',
  deletion_generation INTEGER NOT NULL DEFAULT 0 CHECK (deletion_generation >= 0),
  deleted_at TEXT,
  current_transcript_revision_id TEXT,
  current_summary_revision_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_meetings_recorded_at ON meetings(recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_meetings_active ON meetings(deleted_at);

CREATE TABLE IF NOT EXISTS capture_manifests (
  id TEXT PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  capture_generation INTEGER NOT NULL CHECK (capture_generation >= 0),
  state TEXT NOT NULL,
  manifest_path TEXT NOT NULL,
  media_clock_origin_ns INTEGER NOT NULL DEFAULT 0,
  recovered_at TEXT,
  UNIQUE(meeting_id, capture_generation)
);
CREATE TABLE IF NOT EXISTS media_segments (
  id TEXT PRIMARY KEY,
  manifest_id TEXT NOT NULL REFERENCES capture_manifests(id) ON DELETE CASCADE,
  stream_id TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('system_audio','microphone','screen','mixed')),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  relative_path TEXT NOT NULL,
  start_seconds REAL NOT NULL CHECK (start_seconds >= 0),
  duration_seconds REAL NOT NULL CHECK (duration_seconds >= 0),
  checksum TEXT,
  committed_at TEXT NOT NULL,
  UNIQUE(manifest_id, stream_id, ordinal)
);
CREATE INDEX IF NOT EXISTS idx_media_segments_manifest ON media_segments(manifest_id, ordinal);

CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY,
  meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE,
  document_id TEXT,
  kind TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending','running','done','error','blocked','skipped','canceled')),
  source_revision TEXT,
  config_snapshot TEXT NOT NULL DEFAULT '{}',
  resource_class TEXT NOT NULL DEFAULT 'background',
  cancellation_generation INTEGER NOT NULL DEFAULT 0,
  reason TEXT,
  progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (meeting_id IS NOT NULL OR document_id IS NOT NULL)
);
CREATE INDEX IF NOT EXISTS idx_jobs_owner_state ON jobs(meeting_id, state);
CREATE TABLE IF NOT EXISTS job_attempts (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
  source_revision TEXT,
  config_snapshot TEXT NOT NULL DEFAULT '{}',
  state TEXT NOT NULL,
  progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
  cancellation_generation INTEGER NOT NULL DEFAULT 0,
  error_json TEXT,
  started_at TEXT,
  completed_at TEXT,
  UNIQUE(job_id, id)
);
CREATE INDEX IF NOT EXISTS idx_job_attempts_job ON job_attempts(job_id, started_at);

CREATE TABLE IF NOT EXISTS artifact_revisions (
  id TEXT PRIMARY KEY,
  meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE,
  document_id TEXT REFERENCES documents(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision > 0),
  input_revision_set TEXT NOT NULL DEFAULT '[]',
  model_identity TEXT,
  prompt_identity TEXT,
  config_identity TEXT,
  relative_path TEXT,
  freshness TEXT NOT NULL DEFAULT 'current' CHECK (freshness IN ('current','outdated')),
  published_at TEXT,
  UNIQUE(meeting_id, kind, revision),
  UNIQUE(document_id, kind, revision),
  CHECK ((meeting_id IS NOT NULL) != (document_id IS NOT NULL))
);
CREATE TABLE IF NOT EXISTS transcript_segments (
  id TEXT PRIMARY KEY,
  transcript_revision_id TEXT NOT NULL REFERENCES artifact_revisions(id) ON DELETE CASCADE,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  start_seconds REAL NOT NULL CHECK (start_seconds >= 0),
  end_seconds REAL NOT NULL CHECK (end_seconds >= start_seconds),
  text TEXT NOT NULL,
  language TEXT,
  confidence REAL,
  source_provenance TEXT,
  UNIQUE(transcript_revision_id, ordinal)
);
CREATE INDEX IF NOT EXISTS idx_transcript_time ON transcript_segments(meeting_id, start_seconds);
CREATE TABLE IF NOT EXISTS speakers (
  id TEXT PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  anonymous_label TEXT NOT NULL,
  display_name TEXT,
  color TEXT,
  UNIQUE(meeting_id, anonymous_label)
);
CREATE TABLE IF NOT EXISTS speaker_alignments (
  id TEXT PRIMARY KEY,
  transcript_segment_id TEXT NOT NULL REFERENCES transcript_segments(id) ON DELETE CASCADE,
  speaker_id TEXT NOT NULL REFERENCES speakers(id) ON DELETE CASCADE,
  start_seconds REAL NOT NULL CHECK (start_seconds >= 0),
  end_seconds REAL NOT NULL CHECK (end_seconds >= start_seconds),
  confidence REAL,
  source TEXT NOT NULL,
  uncertainty TEXT
);

CREATE TABLE IF NOT EXISTS slides (
  id TEXT PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  image_hash TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  ocr_text TEXT,
  ocr_revision_id TEXT REFERENCES artifact_revisions(id),
  UNIQUE(meeting_id, image_hash)
);
CREATE TABLE IF NOT EXISTS slide_occurrences (
  id TEXT PRIMARY KEY,
  slide_id TEXT NOT NULL REFERENCES slides(id) ON DELETE CASCADE,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  timestamp_seconds REAL NOT NULL CHECK (timestamp_seconds >= 0),
  UNIQUE(meeting_id, ordinal)
);
CREATE TABLE IF NOT EXISTS summary_revisions (
  id TEXT PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK (revision > 0),
  meeting_type TEXT NOT NULL CHECK (meeting_type IN ('meeting','lecture','generic')),
  structured_json TEXT NOT NULL,
  markdown TEXT NOT NULL,
  input_revision_set TEXT NOT NULL DEFAULT '[]',
  model_identity TEXT,
  prompt_identity TEXT,
  freshness TEXT NOT NULL DEFAULT 'current' CHECK (freshness IN ('current','outdated')),
  published_at TEXT,
  UNIQUE(meeting_id, revision)
);
CREATE TABLE IF NOT EXISTS action_items (
  id TEXT PRIMARY KEY,
  summary_revision_id TEXT NOT NULL REFERENCES summary_revisions(id) ON DELETE CASCADE,
  text TEXT NOT NULL,
  assignee_speaker_id TEXT REFERENCES speakers(id),
  deadline_phrase TEXT,
  normalized_deadline TEXT,
  evidence_refs TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS action_state (
  action_item_id TEXT PRIMARY KEY REFERENCES action_items(id) ON DELETE CASCADE,
  completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0,1)),
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS documents (
  id TEXT PRIMARY KEY,
  content_hash TEXT NOT NULL UNIQUE,
  relative_path TEXT NOT NULL,
  media_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  page_count INTEGER CHECK (page_count IS NULL OR page_count >= 0),
  extraction_state TEXT NOT NULL,
  extraction_version TEXT NOT NULL,
  deleted_at TEXT
);
CREATE TABLE IF NOT EXISTS document_meetings (
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  PRIMARY KEY(document_id, meeting_id)
);
CREATE TABLE IF NOT EXISTS sources (
  id TEXT PRIMARY KEY,
  meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE,
  document_id TEXT REFERENCES documents(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  artifact_revision_id TEXT REFERENCES artifact_revisions(id),
  visibility TEXT NOT NULL DEFAULT 'visible',
  source_revision TEXT NOT NULL,
  CHECK ((meeting_id IS NOT NULL) != (document_id IS NOT NULL))
);
CREATE TABLE IF NOT EXISTS source_chunks (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  chunk_id TEXT NOT NULL UNIQUE,
  text TEXT NOT NULL,
  chunker_version TEXT NOT NULL,
  start_seconds REAL,
  end_seconds REAL,
  page_number INTEGER,
  text_start INTEGER,
  text_end INTEGER,
  UNIQUE(source_id, ordinal)
);
CREATE TABLE IF NOT EXISTS index_generations (
  id TEXT PRIMARY KEY,
  embedding_model_id TEXT NOT NULL,
  embedding_model_revision TEXT NOT NULL,
  dimension INTEGER NOT NULL CHECK (dimension > 0),
  chunker_version TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS index_jobs (
  id TEXT PRIMARY KEY,
  generation_id TEXT NOT NULL REFERENCES index_generations(id) ON DELETE CASCADE,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  source_revision TEXT NOT NULL,
  state TEXT NOT NULL,
  progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
  error_json TEXT,
  updated_at TEXT NOT NULL,
  UNIQUE(generation_id, source_id)
);

CREATE TABLE IF NOT EXISTS chat_threads (
  id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('meeting','all_meetings','documents','everything')),
  meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  CHECK ((scope_kind = 'meeting' AND meeting_id IS NOT NULL) OR (scope_kind != 'meeting' AND meeting_id IS NULL))
);
CREATE TABLE IF NOT EXISTS chat_messages (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  role TEXT NOT NULL CHECK (role IN ('user','assistant','system')),
  content TEXT NOT NULL,
  provider_identity TEXT,
  model_identity TEXT,
  state TEXT NOT NULL,
  request_id TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(thread_id, ordinal)
);
CREATE TABLE IF NOT EXISTS citations (
  id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
  source_chunk_id TEXT REFERENCES source_chunks(id) ON DELETE CASCADE,
  location_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS message_dependencies (
  message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
  depends_on_message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
  PRIMARY KEY(message_id, depends_on_message_id),
  CHECK (message_id != depends_on_message_id)
);

CREATE TABLE IF NOT EXISTS model_assets (
  id TEXT PRIMARY KEY,
  catalog_id TEXT NOT NULL,
  artifact_revision TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  format TEXT NOT NULL,
  role TEXT NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('bundled','imported','downloaded')),
  relative_path TEXT NOT NULL,
  state TEXT NOT NULL,
  UNIQUE(catalog_id, artifact_revision)
);
CREATE TABLE IF NOT EXISTS downloads (
  id TEXT PRIMARY KEY,
  model_asset_id TEXT NOT NULL REFERENCES model_assets(id) ON DELETE CASCADE,
  url TEXT NOT NULL,
  bytes_total INTEGER,
  bytes_received INTEGER NOT NULL DEFAULT 0,
  state TEXT NOT NULL,
  error TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS storage_migrations (
  id TEXT PRIMARY KEY,
  source_root TEXT NOT NULL,
  target_root TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  state TEXT NOT NULL,
  progress REAL NOT NULL DEFAULT 0,
  error TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS cleanup_jobs (
  id TEXT PRIMARY KEY,
  owner_kind TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  generation INTEGER NOT NULL,
  state TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  error TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  schema_version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL
);
