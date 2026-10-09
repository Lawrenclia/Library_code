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
  browsers: Record<string, { open: boolean; usable: boolean }>;
  policy: string;
}
export interface Template {
  id: string;
  name: string;
  path: string;
  sheet: string;
  header_row: number;
  columns: string[];
  required: string[];
  notes: string;
}
