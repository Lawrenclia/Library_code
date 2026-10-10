export interface Failure {
  code: string;
  message: string;
}
export interface Evidence {
  id: string;
  kind: string;
  source: string;
  text: string;
  created: number;
}
export interface Review {
  route: string;
  evidence_id: string;
  library_checked: boolean;
  platform_id: string;
  affiliation_confirmed: boolean;
  identity_confirmed: boolean;
  issues_resolved: boolean;
  note: string;
}
export interface Task {
  id: string;
  input_hash: string;
  paper_id: string;
  revision: number;
  route: string;
  stage: string;
  running: boolean;
  pending_action?: string | null;
  legacy_claim_recovery?: boolean;
  pending_input?: InputProposal | null;
  record: {
    row: number;
    owner: string;
    sa_id: string;
    title: string;
    doi: string;
    wos: string;
    staff_id: string;
    matches: number;
    item_ids: string;
    mark: string;
    reason: string;
    skipped: boolean;
    done: boolean;
    source: string;
  };
  evidence: Evidence[];
  wos_searches?: WosSearchTrace[];
  batch_recheck?: {
    id: string;
    original_input_hash: string;
    original_stage: string;
    created: number;
  } | null;
  last_error: Failure | null;
  review: Review | null;
  classification: Record<string, unknown> | null;
  platform_id: string;
  batch: Record<string, unknown> | null;
  sa_snapshot: Record<string, unknown> | null;
  sa_note?: {
    plan_id: string;
    note: string;
    original_remark: string;
    verified: boolean;
    platform_completed: false;
  } | null;
  issue_plan?: {
    baseline: Record<string, unknown>;
    requirements: { key: string; label: string }[];
  } | null;
  issue_reviews?: {
    key: string;
    outcome: string;
    evidence_id: string;
    note: string;
    snapshot: Record<string, unknown>;
    created: number;
  }[];
  merges?: {
    group_id: string;
    source_id: string;
    target_id: string;
    evidence_id: string;
    created: number;
  }[];
  artifact: null | {
    path: string;
    source: string;
    record_url: string;
    downloaded?: number;
    identity_confirmed: boolean;
    candidate: {
      title: string;
      doi: string;
      wos: string;
      authors: string;
      year: string;
      journal: string;
      affiliation: string;
      sjtu: boolean;
      sha256: string;
      fields: Record<string, string>;
    };
  };
}
export interface WosSearchTrace {
  event_id: number;
  trace_id: string;
  phase: string;
  task_id: string;
  input_hash: string;
  task_snapshot_matches: boolean;
  observed_at: number;
  source_url: string | null;
  data: {
    search_context?: {
      field: string;
      field_label: string;
      query: string;
      scope_controls: string[];
      scope_controls_exhaustive: boolean;
    };
    state?: string;
    outcome?: string;
    error?: Failure;
    [key: string]: unknown;
  };
}
export interface InputProposal {
  id: string;
  record: Task["record"];
  input_hash: string;
  source_file: string;
  previous_fingerprint: string;
  created: number;
}
export interface IssueChecklist {
  requirements: {
    key: string;
    label: string;
    sa: string | null;
    library_before: string | null;
    library: string | null;
  }[];
  baseline: Record<string, unknown>;
  live: Record<string, unknown>;
}
export interface LibrarySearch {
  verified: boolean;
  sa_id: string;
  source: string;
  checked_at: number;
  target: { title: string; doi: string; wos: string };
  queries: {
    kind: string;
    field: string;
    value: string;
    precise: boolean;
    total: number;
    pages: unknown[];
  }[];
  items: {
    id: string;
    modelName?: string;
    metadata: Record<string, unknown> & { title: string[] };
  }[];
}
export interface MetadataPrepared {
  sa: Record<string, unknown>;
  identity: Record<string, unknown>;
  names: string[];
  result: {
    item_id: string;
    staff_id: string;
    scholar: { id: string; wno: string; nameCn: string; nameEn: string };
    authors: {
      index: number;
      id: string;
      fullname: string;
      eligible: boolean;
      fields: string[];
      order: number;
      correspondent: boolean;
      commonFirst: boolean;
    }[];
    can_reorder_authors?: boolean;
    can_reorder_institutions?: boolean;
    institutions?: {
      index: number;
      order: number;
      address: string;
      first_institution_value: string | null;
    }[];
    snapshot: Record<string, unknown>;
  };
}
export interface DuplicateItem {
  id: string;
  model_name: string;
  metadata: Record<string, unknown> & { title: string[] };
}
export interface DuplicateGroup {
  id: string;
  primary_id: string;
  items: DuplicateItem[];
}
export interface DuplicateScan {
  groups: DuplicateGroup[];
  title: string;
  title_similarity: number;
  matched_ids: string[];
}
export interface DuplicatePrepared {
  group: DuplicateGroup;
  title: string;
  title_similarity: number;
  matched_ids: string[];
}
export interface Workspace {
  tasks: Task[];
  root: string;
  running: boolean;
  running_service?:
    "general" | "download" | "ai" | "materials" | "source_download" | null;
  paused: boolean;
  download_queue?: DownloadQueue | null;
  ai_queue?: AiQueue | null;
  material_batch?: MaterialBatch | null;
  browsers: Record<string, { open: boolean; usable: boolean }>;
  policy: string;
  framework?: FrameworkManifest;
  legacy_materials?: {
    root: string;
    roster_hash: string;
    review_required: boolean;
    records: {
      kind: string;
      title: string;
      paper_id: string;
      task_ids: string[];
      warnings: string[];
      manifest: string;
    }[];
    files: {
      original_file: string;
      archive_path: string;
      sha256: string;
      bytes: number;
      format: string;
    }[];
    warnings: string[];
  }[];
}
export interface MaterialBatch {
  id: string;
  owner: string;
  cursor: number;
  status: DownloadQueue["status"];
  pause_requested: boolean;
  targets: { id: string; record: Task["record"]; error: Failure | null }[];
  outcomes: {
    id: string;
    error: Failure | null;
    product: {
      kind: "original" | "original_pending" | "field_valid" | "draft";
      path: string;
      audit: string;
      reused: boolean;
      validation: MaterialValidation;
    } | null;
  }[];
}
export interface AiQueue {
  id: string;
  owner: string;
  zero_only: boolean;
  config: { base: string; model: string };
  template: Record<string, unknown> | null;
  cursor: number;
  targets: {
    id: string;
    record: Task["record"];
    input_hash: string;
    task_revision: number;
    source_hash: string;
  }[];
  outcomes: {
    id: string;
    status: string;
    error: Failure | null;
    finished: number;
  }[];
  status: string;
  pause_requested: boolean;
  inflight: { id: string; task_id: string; task_revision: number } | null;
  last_error: Failure | null;
}
export interface FrameworkManifest {
  schema_version: number;
  services: {
    id: string;
    label: string;
    description: string;
    state: "implemented" | "partial";
    limitation: string;
  }[];
  browsers: { id: string; label: string; url: string; profile: string }[];
  channels: {
    id: string;
    label: string;
    formats: string[];
    automated: boolean;
    live_verified: boolean;
    capabilities: Record<
      string,
      "implemented" | "unimplemented" | "registered_only"
    >;
  }[];
  operations: { id: string; service: string; kind: string }[];
  flows: { id: string; label: string; steps: string[]; outcomes: string[] }[];
  rules: string[];
  live_verified: boolean;
}
export interface DownloadQueue {
  id: string;
  owner: string;
  retry_skipped: boolean;
  status:
    | "running"
    | "paused"
    | "blocked"
    | "interrupted"
    | "completed"
    | "cancelled";
  cursor: number;
  targets: { id: string; fingerprint: string; skipped: boolean }[];
  outcomes: {
    id: string;
    status: string;
    error: { code: string; message: string } | null;
    scope_error?: Failure | null;
    finished: number;
  }[];
  pause_requested: boolean;
  last_error: { code: string; message: string } | null;
}
export interface Template {
  id: string;
  name: string;
  path: string;
  sheet: string;
  header_row: number;
  columns: string[];
  headers?: string[];
  required: string[];
  notes: string;
  field_rules?: {
    column: string;
    source: string;
    check: { kind: string; values?: string[] };
  }[];
}
export interface MaterialProblem {
  column: string;
  reason: string;
  rule_source: string;
}
export interface SubmissionPrepared {
  packet: {
    id: string;
    sa_id: string;
    channel: string;
    channel_label: string;
    work_type: string | null;
    organisation: string;
    instructions: string;
    material: { path: string; sha256: string; kind: string; audit: string };
  };
  task_revision: number;
  can_upload: boolean;
  issues: Failure[];
}
export interface SubmissionOptions {
  sa_id: string;
  task_revision: number;
  choices: {
    recipe: string;
    kind: string;
    path: string;
    sha256: string;
    usable: boolean;
    issue: Failure | null;
    notice?: string;
    channel?: string | null;
    channel_label?: string;
    source_title?: string | null;
    source_location?: string | null;
    source_name?: string | null;
    source_url?: string | null;
    validation?: MaterialValidation;
  }[];
  prepared: SubmissionPrepared | null;
}
export interface MaterialValidation {
  path: string;
  managed_path?: string;
  recipe?: string;
  audit?: string;
  sha256?: string;
  task_revision?: number;
  missing: string[];
  invalid?: MaterialProblem[];
  requires_review?: MaterialProblem[];
  ready?: boolean;
  cancelled?: boolean;
}
export interface SourceDraft {
  options: {
    encoding: string;
    delimiter: string;
    text_table?: boolean;
    tagged_format?: "cnki";
    export_response?: boolean;
  };
  id: string;
  task_id: string;
  task_revision: number;
  channel: string;
  original_name: string;
  path: string;
  sha256: string;
  format: string;
  origin?: {
    task_id: string;
    input_hash: string;
    evidence_id: string;
    evidence_hash: string;
    snapshot_path: string;
    snapshot_hash: string;
    selection: SourceSelection;
    title: string;
    source_url: string;
  };
}
export interface SourceSelection {
  sheet: string;
  header_row: number;
  row: number;
  end_row: number;
  title_column: number | null;
  doi_column: number | null;
  wos_column: number | null;
  text_title: string;
}
export interface SourceReuseChoice {
  source_id: string;
  source_input_hash: string;
  source_fingerprint: string;
  original: {
    evidence_id: string;
    evidence_hash: string;
    receipt: {
      channel: string;
      original_name: string;
      title: string;
      doi: string;
      wos: string;
      source_url: string;
      sha256: string;
      selection: SourceSelection;
    };
  };
}
export interface SourceReuseOptions {
  task_id: string;
  task_revision: number;
  choices: SourceReuseChoice[];
  problems: { source_id: string; error: Failure }[];
}
export interface SourceSite {
  channel: string;
  entry_url: string;
  download_origins: string[];
}
export interface SourceDownload {
  id: string;
  session: { site: SourceSite; record: Task["record"]; input_hash: string };
  original_name: string;
  page_url: string;
  path: string;
  state: string;
  sha256?: string | null;
  bytes?: number | null;
  error?: Failure | null;
}
export interface SourceBrowserState {
  sites: SourceSite[];
  downloads: SourceDownload[];
  windows?: SourceWindowState[];
}
export interface SourceWindowState {
  label: string;
  channel: string;
  channel_label: string;
  session_id: string;
  input_hash: string;
  current_record: boolean;
  current_site: boolean;
  title: string;
  url: string | null;
  popup: boolean;
  downloading: boolean;
  created: number;
}
export interface SourcePage {
  acquisition?: {
    mode: "cnki_export_response";
    source_url: string;
    response_path: string;
    response_sha256: string;
    saved_path: string;
    missing: string[];
  };
  draft: SourceDraft;
  layout?: "text" | "table";
  sheet: string;
  header_row: number;
  page: number;
  total: number;
  actual_encoding: string;
  sheets: { name: string; first_row: number; rows: number }[];
  columns: { column: number; name: string; label: string }[];
  rows: { row: number; values: string[] }[];
  cancelled?: boolean;
}
export interface SourceCandidates {
  schema: "source_table_candidates_v1";
  task_id: string;
  input_hash: string;
  task_revision: number;
  preview_id: string;
  sha256: string;
  sheet: string;
  header_row: number;
  title_column: number;
  doi_column: number | null;
  wos_column: number | null;
  examined: number;
  unreadable: { row: number; error: Failure }[];
  candidates: {
    row: number;
    title: string;
    doi: string;
    wos: string;
    matched_by: string[];
    conflicts: string[];
    basis: "conflict" | "identifier" | "title_only";
    page: number;
  }[];
  automatically_bound: false;
}
