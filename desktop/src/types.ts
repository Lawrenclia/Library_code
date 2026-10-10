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
  paper_id: string;
  revision: number;
  route: string;
  stage: string;
  running: boolean;
  pending_action?: string | null;
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
  last_error: Failure | null;
  review: Review | null;
  classification: Record<string, unknown> | null;
  platform_id: string;
  batch: Record<string, unknown> | null;
  sa_snapshot: Record<string, unknown> | null;
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
  paused: boolean;
  download_queue?: DownloadQueue | null;
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
export interface MaterialValidation {
  path: string;
  task_revision?: number;
  missing: string[];
  invalid?: MaterialProblem[];
  requires_review?: MaterialProblem[];
  ready?: boolean;
  cancelled?: boolean;
}
export interface SourceDraft {
  id: string;
  task_id: string;
  task_revision: number;
  channel: string;
  original_name: string;
  path: string;
  sha256: string;
  format: string;
}
export interface SourcePage {
  draft: SourceDraft;
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
