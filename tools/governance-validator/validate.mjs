#!/usr/bin/env node
// ADR: docs/adr/ADR-0014-validator-structural-parsing-reliability.md

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { relative, resolve, sep } from "node:path";
import { JSDOM } from "jsdom";
import { createAcceptedRecordValidator } from "./lib/accepted-records.mjs";
import { escapeRegExp } from "./lib/escape-regexp.mjs";
import {
  checklistItems,
  hasRequirementLevel,
  headingTexts,
  headings,
  isLightweightAdr,
  isRecordFilename,
  isSeparatorRow,
  sectionContent,
  sectionName,
  sectionTable,
  stripFencedCode,
  stripHtmlComments,
  subsectionContent,
  subsectionTable,
  tableCells,
  tableFromContent,
} from "./lib/markdown-parsing.mjs";
import { createMermaidValidator } from "./lib/mermaid-validation.mjs";
import { createMetadataValidator } from "./lib/metadata-validation.mjs";
import { createRelationshipValidator } from "./lib/relationship-validation.mjs";
import { resolveRepositoryFile } from "./lib/repository-file.mjs";
import { createTerminalValidator } from "./lib/terminal-validation.mjs";

const dom = new JSDOM("<!doctype html><html><body></body></html>");
globalThis.window = dom.window;
globalThis.document = dom.window.document;
const mermaid = (await import("mermaid")).default;
const { metadata, validateUniqueMetadata } = createMetadataValidator({
  stripFencedCode,
  sectionContent,
});

const ADR_DECISION_STATUSES = new Set([
  "Proposed",
  "Accepted",
  "Rejected",
  "Deprecated",
  "Superseded",
]);
const IMPLEMENTATION_STATUSES = new Set([
  "Not Started",
  "In Progress",
  "Blocked",
  "Complete",
  "Verified",
  "Not Applicable",
]);
const ADD_STATUSES = new Set(["Draft", "Current", "Deprecated", "Superseded"]);
// Complete ADD task-candidate status set (AGENTS.md). Any other value is illegal.
const CANDIDATE_STATUSES = new Set(["Ready", "Selected", "Complete", "Deferred"]);
// Valid subtask-level statuses (excludes record-level Verified and Not Applicable).
const SUBTASK_STATUSES = new Set(["Not Started", "In Progress", "Blocked", "Complete"]);
const CHECK_STATUSES = new Set(["Not Started", "In Progress", "Blocked", "Pass", "Fail"]);
const RISK_DIMENSIONS = [
  "concurrency and ordering",
  "timeout and deadline",
  "cancellation and interruption",
  "resource bounds and backpressure",
  "framework or trust-boundary rejection",
];
const IGNORED_DIRECTORIES = new Set([".git", "node_modules", "target"]);

// Required top-level sections per record type, mirroring the `[Required]`
// sections of each template. A Full and a Lightweight ADR share the
// `ADR-NNNN-*` filename, so the ADR branch selects between these two lists
// using the document title (`# Lightweight ADR-NNNN:`).
const FULL_ADR_REQUIRED_SECTIONS = [
  "Metadata",
  "Requirement Level Legend",
  "Context",
  "Scope",
  "Tensions, Constraints, And Open Questions",
  "Decision Drivers",
  "Options Considered",
  "Decision",
  "Implementation Plan",
  "Contract-To-Check Traceability",
  "Risk Coverage Matrix",
  "Acceptance Checks",
  "Completion Checklist",
  "Archival",
  "Change Log",
];
const LIGHTWEIGHT_ADR_REQUIRED_SECTIONS = [
  "Metadata",
  "Requirement Level Legend",
  "Context",
  "Decision",
  "Scope",
  "Lightweight Eligibility Check",
  "Implementation Plan",
  "Contract-To-Check Traceability",
  "Risk Coverage Matrix",
  "Acceptance Checks",
  "Completion Checklist",
  "Archival",
  "Change Log",
];
const OCR_REQUIRED_SECTIONS = [
  "Metadata",
  "Requirement Level Legend",
  "Task Definition",
  "Eligibility",
  "Core Runbook And Evidence",
  "Closure",
  "Archival",
  "Change Log",
];
const OCR_CONDITIONAL_SECTIONS = ["Conditional Extensions"];
const ADD_REQUIRED_SECTIONS = [
  "Metadata",
  "Requirement Level Legend",
  "Context And Solution Summary",
  "Requirement Baseline",
  "Goals And Non-Goals",
  "Functional Capability Design",
  "Architecture Design",
  "Cross-Cutting Design",
  "Risks And Trade-Offs",
  "ADR Task Candidates",
  "Traceability",
  "Approval And Review Checklist",
  "Archival",
  "Change Log",
];
const ADD_CONDITIONAL_SECTIONS = [
  "Data Model Design",
  "Control Flow Design",
  "Interaction Flow Design",
  "Assumptions And Open Questions",
];
function parseArguments(argv) {
  const rootIndex = argv.indexOf("--root");
  if (rootIndex === -1 || !argv[rootIndex + 1]) {
    throw new Error("usage: node validate.mjs --root <repository-root>");
  }
  return resolve(argv[rootIndex + 1]);
}

function repositoryFiles(root) {
  const files = [];
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.isDirectory() && IGNORED_DIRECTORIES.has(entry.name)) continue;
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else files.push(path);
    }
  }
  visit(root);
  return files;
}

function repositoryPath(root, path) {
  return relative(root, path).split(sep).join("/");
}

// Reads a Markdown record through the primary loop's inactive-comment and fenced-code boundaries.
function readActiveMarkdown(path, encoding = "utf8") {
  return stripFencedCode(stripHtmlComments(readFileSync(path, encoding)));
}
function validateRequirementLevels(path, markdown, errors) {
  for (const text of headingTexts(markdown)) {
    if (!hasRequirementLevel(text)) {
      errors.push(`${path}: section ${sectionName(text)} must declare a requirement level`);
    }
  }
}

// A required section is matched by its exact name. Names that templates use as
// a longer alternative form are listed here so the validator never accepts an
// unrelated heading that merely shares a prefix (for example `Decision Drivers`
// must not satisfy `Decision`).
const SECTION_ALIASES = {
  Context: ["Context And Problem Statement"],
};

function sectionCount(found, required) {
  const accepted = new Set([required, ...(SECTION_ALIASES[required] ?? [])]);
  return found.filter((heading) => accepted.has(heading)).length;
}

function validateRequiredSections(path, markdown, required, errors) {
  const found = headings(markdown);
  for (const section of required) {
    const count = sectionCount(found, section);
    if (count === 0) {
      errors.push(`${path}: missing required section ${section}`);
    } else if (count > 1) {
      errors.push(`${path}: duplicate required section ${section}`);
    }
  }
}

function validateStatus(root, path, markdown, errors) {
  validateUniqueMetadata(path, markdown, errors);
  validateRequiredMetadata(path, markdown, errors);
  if (path.includes("/architecture/") && path.split("/").at(-1).startsWith("ADD-")) {
    validateAddStatus(root, path, markdown, errors);
    return;
  }
  const decision = metadata(markdown, "Decision Status");
  const implementation = metadata(markdown, "Implementation Status");
  validateStatusCombination(path, decision, implementation, errors);
  validateAcceptedStages(path, markdown, decision, implementation, errors);
  validateTerminalStages(root, path, markdown, decision, implementation, errors);
}

// Only an Accepted record may enter In Progress or Blocked; a Proposed record
// has not been approved and must remain Not Started (AGENTS.md).
function validateStatusCombination(path, decision, implementation, errors) {
  if (!ADR_DECISION_STATUSES.has(decision)) {
    errors.push(`${path}: illegal Decision Status ${decision ?? "<missing>"}`);
  }
  if (!IMPLEMENTATION_STATUSES.has(implementation)) {
    errors.push(`${path}: illegal Implementation Status ${implementation ?? "<missing>"}`);
  }
  if (["In Progress", "Blocked"].includes(implementation) && decision !== "Accepted") {
    errors.push(`${path}: Implementation Status ${implementation} requires Decision Status Accepted`);
  }
  if (decision === "Proposed" && implementation !== "Not Started") {
    errors.push(`${path}: Proposed requires Implementation Status Not Started`);
  }
}

// Approval evidence, accepted-stage content, and Blocked metadata gates for an
// approved record (AGENTS.md).
function validateAcceptedStages(path, markdown, decision, implementation, errors) {
  if (decision === "Accepted") {
    validateApprovalMetadata(path, markdown, "Accepted", errors);
  }
  if (decision === "Accepted" || ["Complete", "Verified"].includes(implementation)) {
    acceptedRecordValidator.validateRequiredBodyContent(path, markdown, errors);
  }
  if (decision === "Accepted" && implementation === "Blocked") {
    validateBlockedMetadata(path, markdown, errors);
  }
}

// Rejection, retirement, supersession, terminal-evidence, and archival
// residence gates keyed by the record's terminal lifecycle states (AGENTS.md).
function validateTerminalStages(root, path, markdown, decision, implementation, errors) {
  if (decision === "Rejected") {
    validateRejectionMetadata(path, markdown, errors);
    if (implementation !== "Not Applicable") {
      errors.push(`${path}: Rejected requires Implementation Status Not Applicable`);
    }
  }
  if (["Deprecated", "Superseded"].includes(decision)) {
    validateRetirementMetadata(path, markdown, decision, errors);
    if (!["Complete", "Verified", "Not Applicable"].includes(implementation)) {
      errors.push(`${path}: retired decision requires a terminal Implementation Status`);
    }
  }
  if (decision === "Superseded") validateSupersession(root, path, markdown, errors);
  if (["Complete", "Verified"].includes(implementation)) {
    terminalValidator.validateCompletion(path, markdown, errors);
  }
  requireArchived(
    path,
    decision,
    implementation,
    path.split("/").at(-1).startsWith("OCR-"),
    errors,
  );
}

// ADD lifecycle validation: legal Design Status values, approval evidence for
// a Current design, retirement gates, supersession reciprocity, and archival
// residence for retired designs (AGENTS.md).
function validateAddStatus(root, path, markdown, errors) {
  const status = metadata(markdown, "Design Status");
  if (!ADD_STATUSES.has(status)) {
    errors.push(`${path}: illegal Design Status ${status ?? "<missing>"}`);
  }
  if (status === "Current") {
    validateApprovalMetadata(path, markdown, "Current", errors);
    acceptedRecordValidator.validateRequiredBodyContent(path, markdown, errors);
  }
  if (["Deprecated", "Superseded"].includes(status)) {
    validateRetirementMetadata(path, markdown, status, errors);
    validateRetiredCandidates(path, markdown, errors);
  }
  if (status === "Superseded") validateSupersession(root, path, markdown, errors);
  if (["Deprecated", "Superseded"].includes(status) && !path.includes("/archive/")) {
    errors.push(`${path}: a retired (${status}) record must reside under an archive/ directory`);
  }
}

const TIMESTAMP_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:\d{2})$/;
const TIMESTAMP_PARTS_PATTERN = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?(?:Z|([+-])(\d{2}):(\d{2}))$/;

// A concrete `@<actor-id>` identity is required. The identifier must be a
// stable token of allowed characters; role/type labels and template,
// angle-bracket, or `Pending`-style placeholders are rejected.
const FORBIDDEN_ACTOR_LABELS = new Set([
  "human", "agent", "reviewer", "author", "owner", "user", "bot", "system", "ai", "pending",
]);
const ACTOR_ID_PATTERN = /^@[A-Za-z0-9][A-Za-z0-9._-]*$/;

function isValidActor(value) {
  if (typeof value !== "string" || !ACTOR_ID_PATTERN.test(value)) return false;
  return !FORBIDDEN_ACTOR_LABELS.has(value.slice(1).toLowerCase());
}

// A timestamp must be a real ISO 8601 date-time, not merely format-shaped:
// the calendar date must exist and the time and UTC-offset fields must be in
// range, so `2026-99-99T99:99:99+99:99` is rejected.
function isValidTimestamp(value) {
  if (typeof value !== "string" || !TIMESTAMP_PATTERN.test(value)) return false;
  const match = TIMESTAMP_PARTS_PATTERN.exec(value);
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = match[6] === undefined ? 0 : Number(match[6]);
  if (hour > 23 || minute > 59 || second > 59) return false;
  if (match[7] !== undefined && (Number(match[8]) > 23 || Number(match[9]) > 59)) return false;
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    date.getUTCFullYear() === year
    && date.getUTCMonth() === month - 1
    && date.getUTCDate() === day
  );
}

// A concrete required value: non-empty and not a Pending, N/A, or template
// placeholder. Used for triggered conditionally-required fields that must not
// remain placeholders once their stage is active.
function isCompleteValue(value) {
  if (typeof value !== "string") return false;
  const trimmed = value.trim();
  if (!trimmed) return false;
  if (/^Pending\b/i.test(trimmed)) return false;
  if (/^N\/A\b/i.test(trimmed)) return false;
  if (/\{\{[A-Z0-9_]+\}\}/.test(trimmed)) return false;
  return true;
}

// A table cell may legitimately start with the word "Pending" when it
// describes a state under test. Only a standalone lifecycle placeholder is
// incomplete at the approval gate.
function isCompleteTableValue(value) {
  if (typeof value !== "string") return false;
  const trimmed = value.trim();
  if (!trimmed) return false;
  if (/^Pending(?:\s+—.*)?$/i.test(trimmed)) return false;
  if (/^N\/A\b/i.test(trimmed)) return false;
  if (/\{\{[A-Z0-9_]+\}\}/.test(trimmed)) return false;
  return true;
}

function validateApprovalMetadata(path, markdown, status, errors) {
  const author = metadata(markdown, "Author");
  const approver = metadata(markdown, "Approver");
  const approvalTime = metadata(markdown, "Approval Time");
  const approvalEvidence = metadata(markdown, "Approval Evidence");
  if (!isValidActor(approver) || !isValidTimestamp(approvalTime) || approvalEvidence !== "Approve") {
    errors.push(`${path}: ${status} requires complete Approver, Approval Time, and Approval Evidence: Approve metadata`);
  }
  // The author or drafting agent may not approve the same document.
  if (isValidActor(approver) && isValidActor(author) && approver === author) {
    errors.push(`${path}: ${status} Approver must differ from Author`);
  }
}

// An Accepted record that enters Blocked must record its prior status, the
// specific blocker and evidence, a concrete blocker owner, and a deterministic
// exit or recheck criterion (AGENTS.md). Triggered content must be complete,
// not Pending/N/A/placeholder.
function validateBlockedMetadata(path, markdown, errors) {
  const blockedFrom = metadata(markdown, "Blocked From");
  const blockerAndEvidence = metadata(markdown, "Blocker And Evidence");
  const blockerOwner = metadata(markdown, "Blocker Owner");
  const blockerExit = metadata(markdown, "Blocker Exit Or Recheck Criterion");
  if (
    !["Not Started", "In Progress"].includes(blockedFrom)
    || !isCompleteValue(blockerAndEvidence)
    || !isValidActor(blockerOwner)
    || !isCompleteValue(blockerExit)
  ) {
    errors.push(
      `${path}: Blocked requires complete Blocked From (Not Started or In Progress), Blocker And Evidence, Blocker Owner, and Blocker Exit Or Recheck Criterion metadata`,
    );
  }
}

// A Rejected record must carry the rejecting actor, time, and exact `Reject`
// evidence so an unauthorized state transition cannot pass CI.
function validateRejectionMetadata(path, markdown, errors) {
  const rejector = metadata(markdown, "Rejector");
  const rejectionTime = metadata(markdown, "Rejection Time");
  const rejectionEvidence = metadata(markdown, "Rejection Evidence");
  if (!isValidActor(rejector) || !isValidTimestamp(rejectionTime) || rejectionEvidence !== "Reject") {
    errors.push(`${path}: Rejected requires complete Rejector, Rejection Time, and Rejection Evidence: Reject metadata`);
  }
}

// A Deprecated or Superseded record must carry the retiring actor, time, the
// retirement evidence matching the target status (`Deprecate` for Deprecated,
// `Supersede` for Superseded), and a retirement reason.
function validateRetirementMetadata(path, markdown, decision, errors) {
  const expectedEvidence = decision === "Superseded" ? "Supersede" : "Deprecate";
  const retiredBy = metadata(markdown, "Retired By");
  const retirementTime = metadata(markdown, "Retirement Time");
  const retirementEvidence = metadata(markdown, "Retirement Evidence");
  const retirementReason = metadata(markdown, "Retirement Reason");
  if (
    !isValidActor(retiredBy)
    || !isValidTimestamp(retirementTime)
    || retirementEvidence !== expectedEvidence
    || !isCompleteValue(retirementReason)
  ) {
    errors.push(
      `${path}: retired decision requires complete Retired By, Retirement Time, Retirement Evidence: ${expectedEvidence}, and Retirement Reason metadata`,
    );
  }
}

// Every document must identify its actors and scope; OCRs additionally declare
// their operation, target/owner, immutable input, and expected state. Requiring
// Author also prevents bypassing the author/approver self-approval check.
function validateRequiredMetadata(path, markdown, errors) {
  const filename = path.split("/").at(-1);
  const isAdd = path.includes("/architecture/") && filename.startsWith("ADD-");
  const isOcr = filename.startsWith("OCR-");
  const ownerField = isAdd ? "Architecture Owner" : "Decision Owner";
  const actorFields = ["Author", ownerField, "Required Approver"];
  for (const field of actorFields) {
    if (!isValidActor(metadata(markdown, field))) {
      errors.push(`${path}: metadata field ${field} must be a concrete @<actor-id>`);
    }
  }
  for (const field of requiredScopeFields(isAdd, isOcr)) {
    if (!isCompleteValue(metadata(markdown, field))) {
      errors.push(`${path}: missing required metadata field ${field}`);
    }
  }
}

// Scope-defining fields beyond actor identity, keyed by record type: ADDs
// declare design scope, OCRs declare operation scope, ADRs declare record
// scope only.
function requiredScopeFields(isAdd, isOcr) {
  if (isAdd) return ["Scope Level", "Scope"];
  if (isOcr) {
    return [
      "Record Scope", "Operation Type", "Target Scope / Operation Owner",
      "Input Source or Version", "Expected Output or Target State",
    ];
  }
  return ["Record Scope"];
}

// Before an ADD retires, every task candidate must be Deferred or Complete
// (AGENTS.md); a retired ADD cannot leave a Ready or Selected candidate behind.

function validateRetiredCandidates(path, markdown, errors) {
  const parsed = relationshipValidator.candidateTable(markdown);
  if (!parsed) return; // a missing/malformed table is reported elsewhere
  for (const [candidate, { status }] of parsed.table) {
    if (status !== "Deferred" && status !== "Complete") {
      errors.push(
        `${path}: retired ADD candidate ${candidate} must be Deferred or Complete (is ${status})`,
      );
    }
  }
}

// Determines whether text after a record path leaves its Markdown boundary intact.
function hasRecordPathBoundary(value, index) {
  const next = value[index];
  return next === undefined || [" ", "\t", "|", "`"].includes(next) || (next === "." && (index + 1 === value.length || [" ", "\t"].includes(value[index + 1])));
}

// Extracts a safely delimited repository-relative ADD, ADR, or OCR path.
function recordPathFromValue(value) {
  if (typeof value !== "string") return undefined;
  const start = value.indexOf("docs/");
  if (start === -1) return undefined;
  for (let index = start; index <= value.length - 3; index += 1) {
    if (!value.startsWith(".md", index)) continue;
    const end = index + 3;
    const candidate = value.slice(start, end);
    if ((candidate.startsWith("docs/adr/") || candidate.startsWith("docs/architecture/")) && ![" ", "\t", "|", "`"].some((character) => candidate.includes(character)) && hasRecordPathBoundary(value, end)) return candidate;
  }
  return undefined;
}
// A Superseded record must name its replacement (`Superseded By`), the
// replacement must be Accepted (ADR/OCR) or Current (ADD), and the replacement
// must reciprocally `Supersede` this exact record (AGENTS.md).
function validateSupersession(root, path, markdown, errors) {
  const raw = metadata(markdown, "Superseded By");
  const replacementPath = recordPathFromValue(raw);
  if (!replacementPath) {
    errors.push(`${path}: Superseded requires a Superseded By replacement record path`);
    return;
  }
  // The replacement must be a real, indexed ADR/ADD/OCR record — not a
  // translations helper or arbitrary markdown file that merely declares a status.
  const filename = replacementPath.slice(replacementPath.lastIndexOf("/") + 1);
  if (
    replacementPath.includes("/translations/")
    || !["ADR-", "ADD-", "OCR-"].some((prefix) => isRecordFilename(filename, prefix))
  ) {
    errors.push(`${path}: Superseded By must name an ADR, ADD, or OCR record: ${replacementPath}`);
    return;
  }
  const indexPath = replacementPath.includes("/architecture/")
    ? "docs/architecture/INDEX.md"
    : "docs/adr/INDEX.md";
  const indexAbsolute = resolve(root, indexPath);
  const index = existsSync(indexAbsolute) ? readFileSync(indexAbsolute, "utf8") : "";
  if (!index.includes(replacementPath)) {
    errors.push(`${path}: Superseded By replacement ${replacementPath} is not in the index`);
    return;
  }
  const absolute = resolveRepositoryFile(
    root,
    replacementPath,
    path,
    "Superseded By replacement path",
    errors,
  );
  if (!absolute) return;
  const replacement = readFileSync(absolute, "utf8");
  const isAdd = replacementPath.includes("/architecture/");
  const status = metadata(replacement, isAdd ? "Design Status" : "Decision Status");
  const requiredStatus = isAdd ? "Current" : "Accepted";
  if (status !== requiredStatus) {
    errors.push(
      `${path}: Superseded By replacement ${replacementPath} must be ${requiredStatus} (is ${status ?? "<missing>"})`,
    );
  }
  const supersedes = recordPathFromValue(metadata(replacement, "Supersedes"));
  if (supersedes !== path) {
    errors.push(
      `${path}: Superseded By replacement ${replacementPath} does not reciprocally Supersede this record`,
    );
  }
}

// A record reaching an archival-eligible state must have been moved into its
// archive directory: retired (Deprecated/Superseded) records, a Rejected ADR
// (Rejected/Not Applicable), and an OCR whose implementation reached a final
// state (Verified, Complete, Not Applicable). A Blocked OCR is not archived
// solely on its status: it may still be re-attempted under its recorded exit
// criterion.
function requireArchived(path, decision, implementation, isOcr, errors) {
  const required = isOcr
    ? ["Deprecated", "Superseded"].includes(decision)
      || ["Verified", "Complete", "Not Applicable"].includes(implementation)
    : ["Deprecated", "Superseded"].includes(decision)
      || (decision === "Rejected" && implementation === "Not Applicable");
  if (required && !path.includes("/archive/")) {
    errors.push(
      `${path}: a ${decision}/${implementation} record must reside under an archive/ directory`,
    );
  }
}

// A terminal implementation status requires actual completion evidence. The
// record type determines which tables carry that evidence: ADRs use the
// Implementation Plan, Acceptance Checks, Completion Checklist, and Risk
// Coverage Matrix; OCRs use the Task Definition subtasks, Core Runbook stages,
// and Closure review fields (AGENTS.md).
function normalizeDimension(value) {
  return value.toLowerCase().replaceAll("-", "").replace(/\s+/g, " ").trim();
}

function validateTemplateVariables(path, markdown, isTemplate, errors) {
  const variables = [...markdown.matchAll(/{{([A-Z0-9_]+)}}/g)].map((match) => match[1]);
  if (!isTemplate) {
    for (const variable of new Set(variables)) {
      errors.push(`${path}: unresolved template variable ${variable}`);
    }
    return;
  }

  const declared = new Set(
    [...markdown.matchAll(/^\|\s*`{{([A-Z0-9_]+)}}`\s*\|/gm)].map((match) => match[1]),
  );
  for (const variable of new Set(variables)) {
    if (!declared.has(variable)) errors.push(`${path}: template variable ${variable} is not declared`);
  }
  for (const variable of declared) {
    if (variables.filter((candidate) => candidate === variable).length < 2) {
      errors.push(`${path}: declared template variable ${variable} is not used`);
    }
  }
}

const acceptedRecordValidator = createAcceptedRecordValidator({
  ADD_CONDITIONAL_SECTIONS,
  ADD_REQUIRED_SECTIONS,
  CHECK_STATUSES,
  FULL_ADR_REQUIRED_SECTIONS,
  LIGHTWEIGHT_ADR_REQUIRED_SECTIONS,
  OCR_CONDITIONAL_SECTIONS,
  OCR_REQUIRED_SECTIONS,
  RISK_DIMENSIONS,
  SUBTASK_STATUSES,
  checklistItems,
  isCompleteTableValue,
  metadata,
  normalizeDimension,
  sectionContent,
  sectionTable,
  stripFencedCode,
  subsectionContent,
  subsectionTable,
  isCompleteValue,
  isSeparatorRow,
  tableCells,
});

const terminalValidator = createTerminalValidator({
  RISK_DIMENSIONS,
  isCompleteValue,
  normalizeDimension,
  sectionContent,
  sectionTable,
  stripFencedCode,
  subsectionContent,
});

const relationshipValidator = createRelationshipValidator({
  CANDIDATE_STATUSES,
  metadata,
  readFileSync: readActiveMarkdown,
  resolveRepositoryFile,
  sectionContent,
  tableFromContent,
});

const mermaidValidator = createMermaidValidator({
  escapeRegExp,
  mermaid,
  metadata,
  sectionContent,
  stripFencedCode,
  tableFromContent,
});

// Validates one recognized ADR record's structural, lifecycle, source, and
// risk-dimension contracts.
function validateAdrRecord(root, path, markdown, errors) {
  const requiredSections = isLightweightAdr(markdown)
    ? LIGHTWEIGHT_ADR_REQUIRED_SECTIONS
    : FULL_ADR_REQUIRED_SECTIONS;
  validateRequirementLevels(path, markdown, errors);
  validateRequiredSections(path, markdown, requiredSections, errors);
  validateStatus(root, path, markdown, errors);
  relationshipValidator.validateArchitectureSource(root, path, markdown, errors);
  acceptedRecordValidator.validateRiskMatrixDimensions(path, markdown, errors);
}

// Validates one recognized OCR record's structural and lifecycle contracts.
function validateOcrRecord(root, path, markdown, errors) {
  validateRequirementLevels(path, markdown, errors);
  validateRequiredSections(path, markdown, OCR_REQUIRED_SECTIONS, errors);
  validateStatus(root, path, markdown, errors);
}

// Validates one recognized ADD record's structural, lifecycle, reciprocal-link,
// and Mermaid diagram contracts.
async function validateAddRecord(root, path, markdown, errors) {
  validateRequirementLevels(path, markdown, errors);
  validateRequiredSections(path, markdown, ADD_REQUIRED_SECTIONS, errors);
  validateStatus(root, path, markdown, errors);
  relationshipValidator.validateReciprocalLinks(root, path, markdown, errors);
  await mermaidValidator.validateMermaid(path, markdown, errors);
}

// Validates one repository Markdown file as an index, translation page, or
// governance record, registering recognized records for index-coverage checks.
async function validateMarkdownFile(root, path, rawMarkdown, indexed, records, errors) {
  const markdown = stripHtmlComments(rawMarkdown);
  const isTemplate = path.includes("/template/") || path === "AGENTS.template.md";
  // Template variable declarations intentionally live in authoring-only
  // HTML comments; validate that dedicated contract from raw template text.
  validateTemplateVariables(path, rawMarkdown, isTemplate, errors);

  if (path.endsWith("/INDEX.md")) {
    const paths = relationshipValidator.validateIndex(root, path, markdown, errors);
    if (path === "docs/adr/INDEX.md") indexed.adr = paths;
    if (path === "docs/architecture/INDEX.md") indexed.architecture = paths;
  }
  if (path.includes("/translations/")) {
    if (markdown.includes("```mermaid")) await mermaidValidator.validateMermaid(path, markdown, errors);
    return;
  }

  const filename = path.split("/").at(-1);
  if (isRecordFilename(filename, "ADR-")) {
    records.push({ path, index: "adr" });
    validateAdrRecord(root, path, markdown, errors);
  } else if (isRecordFilename(filename, "OCR-")) {
    records.push({ path, index: "adr" });
    validateOcrRecord(root, path, markdown, errors);
  } else if (isRecordFilename(filename, "ADD-")) {
    records.push({ path, index: "architecture" });
    await validateAddRecord(root, path, markdown, errors);
  }
}

async function validate(root) {
  mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
  const errors = [];
  const files = repositoryFiles(root);
  const markdownFiles = files.filter((path) => path.endsWith(".md"));
  const indexed = {
    adr: new Set(),
    architecture: new Set(),
  };
  const records = [];

  for (const absolute of markdownFiles) {
    const path = repositoryPath(root, absolute);
    const rawMarkdown = readFileSync(absolute, "utf8");
    await validateMarkdownFile(root, path, rawMarkdown, indexed, records, errors);
  }
  for (const record of records) {
    const indexPath = record.index === "adr" ? "docs/adr/INDEX.md" : "docs/architecture/INDEX.md";
    if (!indexed[record.index].has(record.path)) {
      errors.push(`${record.path}: record is missing from ${indexPath}`);
    }
  }
  return errors;
}

try {
  const root = parseArguments(process.argv.slice(2));
  const errors = await validate(root);
  if (errors.length > 0) {
    for (const error of errors) process.stderr.write(`${error}\n`);
    process.exitCode = 1;
  } else {
    process.stdout.write("Governance validation passed.\n");
  }
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 2;
}
