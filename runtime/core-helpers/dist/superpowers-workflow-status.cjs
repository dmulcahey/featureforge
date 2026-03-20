"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/cli/superpowers-workflow-status.ts
var superpowers_workflow_status_exports = {};
__export(superpowers_workflow_status_exports, {
  main: () => main
});
module.exports = __toCommonJS(superpowers_workflow_status_exports);
var import_node_path4 = __toESM(require("node:path"), 1);

// src/core/workflow-status.ts
var import_node_crypto = __toESM(require("node:crypto"), 1);
var import_node_fs2 = __toESM(require("node:fs"), 1);
var import_node_os2 = __toESM(require("node:os"), 1);
var import_node_path3 = __toESM(require("node:path"), 1);
var import_node_child_process = require("node:child_process");

// src/platform/filesystem.ts
var import_node_fs = __toESM(require("node:fs"), 1);
var import_node_path = __toESM(require("node:path"), 1);
function pathExists(filePath) {
  return import_node_fs.default.existsSync(filePath);
}
function isFilePath(filePath) {
  try {
    return import_node_fs.default.statSync(filePath).isFile();
  } catch {
    return false;
  }
}
function ensureDirectoryExists(directoryPath) {
  import_node_fs.default.mkdirSync(directoryPath, { recursive: true });
}
function readTextFileIfExists(filePath) {
  if (!pathExists(filePath)) {
    return "";
  }
  return import_node_fs.default.readFileSync(filePath, "utf8");
}
function movePath(sourcePath, destinationPath) {
  ensureDirectoryExists(import_node_path.default.dirname(destinationPath));
  import_node_fs.default.renameSync(sourcePath, destinationPath);
}
function listNewestFiles(directoryPath, options = {}) {
  if (!pathExists(directoryPath)) {
    return [];
  }
  const extension = options.extension ?? "";
  const limit = options.limit ?? Number.MAX_SAFE_INTEGER;
  return import_node_fs.default.readdirSync(directoryPath).filter((entry) => extension.length > 0 ? entry.endsWith(extension) : true).map((entry) => import_node_path.default.join(directoryPath, entry)).filter((entryPath) => isFilePath(entryPath)).sort((left, right) => {
    const timeDifference = import_node_fs.default.statSync(right).mtimeMs - import_node_fs.default.statSync(left).mtimeMs;
    if (timeDifference !== 0) {
      return timeDifference;
    }
    return left.localeCompare(right);
  }).slice(0, limit);
}

// src/platform/paths.ts
var import_node_os = __toESM(require("node:os"), 1);
var import_node_path2 = __toESM(require("node:path"), 1);
function resolveFromRuntimeRoot(runtimeRoot, relativePath) {
  return import_node_path2.default.resolve(runtimeRoot, relativePath);
}
function resolveRuntimeRoot(entryPath, runtimeRootOverride) {
  if (runtimeRootOverride && runtimeRootOverride.length > 0) {
    return import_node_path2.default.resolve(runtimeRootOverride);
  }
  return import_node_path2.default.resolve(import_node_path2.default.dirname(entryPath), "../../..");
}
function resolveStateDir(env) {
  if (env.SUPERPOWERS_STATE_DIR && env.SUPERPOWERS_STATE_DIR.length > 0) {
    return env.SUPERPOWERS_STATE_DIR;
  }
  if (env.HOME && env.HOME.length > 0) {
    return import_node_path2.default.join(env.HOME, ".superpowers");
  }
  if (env.USERPROFILE && env.USERPROFILE.length > 0) {
    return import_node_path2.default.join(env.USERPROFILE, ".superpowers");
  }
  if (env.HOMEDRIVE && env.HOMEPATH && env.HOMEDRIVE.length > 0 && env.HOMEPATH.length > 0) {
    return import_node_path2.default.join(`${env.HOMEDRIVE}${env.HOMEPATH}`, ".superpowers");
  }
  return import_node_path2.default.join(import_node_os.default.homedir(), ".superpowers");
}
function normalizeRelativePath(input) {
  if (input.length === 0 || import_node_path2.default.isAbsolute(input)) {
    return null;
  }
  const normalizedParts = [];
  for (const part of input.replace(/\\/g, "/").split("/")) {
    if (part === "" || part === ".") {
      continue;
    }
    if (part === "..") {
      return null;
    }
    normalizedParts.push(part);
  }
  if (normalizedParts.length === 0) {
    return null;
  }
  return normalizedParts.join("/");
}
function isPathInsideRoot(rootPath, candidatePath) {
  const relativePath = import_node_path2.default.relative(rootPath, candidatePath);
  return relativePath === "" || !relativePath.startsWith("..") && !import_node_path2.default.isAbsolute(relativePath);
}

// src/core/workflow-status.ts
function normalizeLineEndings(text) {
  return text.replace(/\r\n/g, "\n");
}
function firstMatch(text, pattern) {
  for (const line of normalizeLineEndings(text).split("\n")) {
    const match = line.match(pattern);
    if (match) {
      return match[1] ?? "";
    }
  }
  return "";
}
function parseStateString(value, allowed) {
  return allowed.includes(value) ? value : null;
}
function normalizeRepoRelativePath(input) {
  return normalizeRelativePath(input);
}
function parseSpecHeaders(text) {
  const state = parseStateString(
    firstMatch(text, /^\*\*Workflow State:\*\* (Draft|CEO Approved)$/),
    ["Draft", "CEO Approved"]
  );
  const revision = firstMatch(text, /^\*\*Spec Revision:\*\* ([0-9]+)$/);
  const reviewer = parseStateString(
    firstMatch(text, /^\*\*Last Reviewed By:\*\* (brainstorming|plan-ceo-review)$/),
    ["brainstorming", "plan-ceo-review"]
  );
  if (state === null || revision.length === 0 || reviewer === null) {
    return null;
  }
  return {
    state,
    revision,
    reviewer
  };
}
function parsePlanHeaders(text) {
  const state = parseStateString(
    firstMatch(text, /^\*\*Workflow State:\*\* (Draft|Engineering Approved)$/),
    ["Draft", "Engineering Approved"]
  );
  const rawSourceSpec = firstMatch(text, /^\*\*Source Spec:\*\* (.+)$/);
  const sourceRevision = firstMatch(text, /^\*\*Source Spec Revision:\*\* ([0-9]+)$/);
  const reviewer = parseStateString(
    firstMatch(text, /^\*\*Last Reviewed By:\*\* (writing-plans|plan-eng-review)$/),
    ["writing-plans", "plan-eng-review"]
  );
  const sourceSpec = rawSourceSpec.replace(/^`/, "").replace(/`$/, "");
  if (state === null || sourceSpec.length === 0 || sourceRevision.length === 0 || reviewer === null) {
    return null;
  }
  return {
    state,
    sourceSpec,
    sourceRevision,
    reviewer
  };
}
function classifyWorkflowState(input) {
  const reasons = [];
  if (input.specAmbiguous) {
    reasons.push("fallback_ambiguity_spec");
    return {
      statusCode: "spec_draft",
      nextSkill: "superpowers:plan-ceo-review",
      reason: reasons.join(",")
    };
  }
  if (input.resolvedSpecPath.length === 0) {
    if (input.specExpectedMissing) {
      reasons.push("missing_expected_spec");
    }
    return {
      statusCode: "needs_brainstorming",
      nextSkill: "superpowers:brainstorming",
      reason: reasons.join(",")
    };
  }
  if (!input.specExists) {
    if (input.specExpectedMissing) {
      reasons.push("missing_expected_spec");
    }
    return {
      statusCode: "needs_brainstorming",
      nextSkill: "superpowers:brainstorming",
      reason: reasons.join(",")
    };
  }
  if (input.specHeaders === null) {
    reasons.push("malformed_spec_headers");
    return {
      statusCode: "spec_draft",
      nextSkill: "superpowers:plan-ceo-review",
      reason: reasons.join(",")
    };
  }
  if (input.specHeaders.state === "Draft") {
    return {
      statusCode: "spec_draft",
      nextSkill: "superpowers:plan-ceo-review",
      reason: reasons.join(",")
    };
  }
  if (input.planAmbiguous) {
    reasons.push("fallback_ambiguity_plan");
    return {
      statusCode: "spec_approved_needs_plan",
      nextSkill: "superpowers:writing-plans",
      reason: reasons.join(",")
    };
  }
  if (input.resolvedPlanPath.length === 0) {
    if (input.planExpectedMissing) {
      reasons.push("missing_expected_plan");
    }
    return {
      statusCode: "spec_approved_needs_plan",
      nextSkill: "superpowers:writing-plans",
      reason: reasons.join(",")
    };
  }
  if (!input.planExists) {
    if (input.planExpectedMissing) {
      reasons.push("missing_expected_plan");
    }
    return {
      statusCode: "spec_approved_needs_plan",
      nextSkill: "superpowers:writing-plans",
      reason: reasons.join(",")
    };
  }
  if (input.planHeaders === null) {
    reasons.push("malformed_plan_headers");
    return {
      statusCode: "plan_draft",
      nextSkill: "superpowers:plan-eng-review",
      reason: reasons.join(",")
    };
  }
  if (input.planHeaders.state === "Draft") {
    return {
      statusCode: "plan_draft",
      nextSkill: "superpowers:plan-eng-review",
      reason: reasons.join(",")
    };
  }
  const normalizedSource = normalizeRepoRelativePath(input.planHeaders.sourceSpec);
  if (normalizedSource === null || normalizedSource !== input.resolvedSpecPath || input.planHeaders.sourceRevision !== input.specHeaders.revision) {
    return {
      statusCode: "stale_plan",
      nextSkill: "superpowers:writing-plans",
      reason: ""
    };
  }
  reasons.push("implementation_ready");
  return {
    statusCode: "implementation_ready",
    nextSkill: "",
    reason: reasons.join(",")
  };
}
var WorkflowStatusRunner = class {
  cwd;
  env;
  runtimeRoot;
  stateDir;
  repoRoot;
  repoContextAvailable;
  branch;
  safeBranch;
  slug;
  userName;
  manifestPath;
  fallbackLimit;
  recoveryCandidateLimit = 12;
  stdoutChunks = [];
  stderrChunks = [];
  manifestSpecPath = "";
  manifestPlanPath = "";
  manifestStatus = "";
  manifestNextSkill = "";
  manifestReason = "";
  manifestRepoRoot = "";
  manifestBranch = "";
  manifestSourcePath = "";
  manifestValid = false;
  manifestCorrupt = false;
  manifestRecovered = false;
  manifestRecoveryAmbiguous = false;
  manifestIdentityMismatch = false;
  manifestIdentityReason = "";
  resolvedSpecPath = "";
  resolvedPlanPath = "";
  specHeaders = null;
  planHeaders = null;
  statusCode = "needs_brainstorming";
  nextSkill = "superpowers:brainstorming";
  statusNotes = [];
  specFromManifest = false;
  specExpectedMissing = false;
  specAmbiguous = false;
  planFromManifest = false;
  planExpectedMissing = false;
  planAmbiguous = false;
  constructor(options) {
    this.cwd = import_node_path3.default.resolve(options.cwd ?? process.cwd());
    this.env = options.env ?? process.env;
    this.runtimeRoot = options.runtimeRoot ?? this.cwd;
    this.stateDir = resolveStateDir(this.env);
    const repoRootResult = this.runGitCommand(["rev-parse", "--show-toplevel"]);
    if (repoRootResult.success) {
      this.repoRoot = repoRootResult.stdout;
      this.repoContextAvailable = true;
    } else {
      this.repoRoot = this.cwd;
      this.repoContextAvailable = false;
    }
    const rawBranch = this.readBranchName();
    this.branch = rawBranch;
    this.userName = this.readUserName();
    this.fallbackLimit = this.readFallbackLimit();
    const runtimeSlug = this.readRuntimeSlug(rawBranch);
    this.slug = runtimeSlug.slug;
    this.safeBranch = runtimeSlug.safeBranch;
    this.manifestPath = import_node_path3.default.join(
      this.stateDir,
      "projects",
      this.slug,
      `${this.userName}-${this.safeBranch}-workflow-state.json`
    );
  }
  run(args) {
    const [command, ...commandArgs] = args;
    switch (command) {
      case "status":
        return this.commandStatus(commandArgs);
      case "resolve":
        return this.commandResolve(commandArgs);
      case "expect":
        return this.commandExpect(commandArgs);
      case "sync":
        return this.commandSync(commandArgs);
      default:
        this.writeStdout(this.usageText());
        return this.result(1);
    }
  }
  result(exitCode) {
    return {
      exitCode,
      stdout: this.stdoutChunks.join(""),
      stderr: this.stderrChunks.join("")
    };
  }
  usageText() {
    return "Usage: superpowers-workflow-status {status|resolve|expect|sync} ...\n";
  }
  writeStdout(text) {
    this.stdoutChunks.push(text);
  }
  writeStderr(text) {
    this.stderrChunks.push(text);
  }
  appendStatusNote(note) {
    if (note.length > 0) {
      this.statusNotes.push(note);
    }
  }
  setStatusNotes(noteText) {
    this.statusNotes = noteText.length > 0 ? [noteText] : [];
  }
  currentReason() {
    return this.statusNotes.join(",");
  }
  emitStatusJson(statusCode, nextSkill, specPath, planPath, reason = "") {
    const payload = {
      status: statusCode,
      next_skill: nextSkill,
      spec_path: specPath,
      plan_path: planPath,
      manifest_path: this.manifestPath,
      root: this.repoRoot
    };
    if (reason.length > 0) {
      payload.reason = reason;
      payload.note = reason;
    }
    this.writeStdout(`${JSON.stringify(payload)}
`);
  }
  emitResolveJson(statusCode, nextSkill, specPath, planPath, reason = "") {
    const payload = {
      outcome: "resolved",
      status: statusCode,
      next_skill: nextSkill,
      spec_path: specPath,
      plan_path: planPath,
      manifest_path: this.manifestPath,
      manifest_source_path: this.manifestSourcePath,
      root: this.repoRoot
    };
    if (reason.length > 0) {
      payload.reason = reason;
      payload.note = reason;
    }
    this.writeStdout(`${JSON.stringify(payload)}
`);
  }
  emitResolveFailure(failureClass, message) {
    const payload = {
      outcome: "runtime_failure",
      failure_class: failureClass,
      message,
      manifest_path: this.manifestPath,
      root: this.repoRoot
    };
    this.writeStdout(`${JSON.stringify(payload)}
`);
  }
  emitSummary(statusCode, nextSkill, specPath, planPath, reason = "") {
    const nextDisplay = statusCode === "implementation_ready" && nextSkill.length === 0 ? "execution_handoff" : nextSkill;
    this.writeStdout(
      `status=${statusCode} next=${nextDisplay} spec=${specPath} plan=${planPath} reason=${reason}
`
    );
  }
  ensureManifestDir() {
    ensureDirectoryExists(import_node_path3.default.dirname(this.manifestPath));
  }
  readUserName() {
    const envUser = this.env.USER ?? this.env.USERNAME;
    if (envUser && envUser.length > 0) {
      return envUser;
    }
    try {
      return import_node_os2.default.userInfo().username;
    } catch {
      return "user";
    }
  }
  readFallbackLimit() {
    const raw = this.env.SUPERPOWERS_WORKFLOW_STATUS_FALLBACK_LIMIT ?? "";
    if (!/^[1-9][0-9]*$/.test(raw)) {
      return 6;
    }
    return Number.parseInt(raw, 10);
  }
  runGitCommand(args) {
    const result = (0, import_node_child_process.spawnSync)("git", args, {
      cwd: this.cwd,
      encoding: "utf8"
    });
    if (result.status !== 0) {
      return {
        success: false,
        stdout: ""
      };
    }
    return {
      success: true,
      stdout: result.stdout.trim()
    };
  }
  readBranchName() {
    const branchResult = this.runGitCommand(["rev-parse", "--abbrev-ref", "HEAD"]);
    const rawBranch = branchResult.success && branchResult.stdout.length > 0 ? branchResult.stdout : "current";
    return rawBranch === "HEAD" ? "current" : rawBranch;
  }
  hashRepoRoot() {
    return import_node_crypto.default.createHash("sha256").update(this.repoRoot).digest("hex").slice(0, 12);
  }
  deriveRepoSlug(remoteUrl) {
    if (remoteUrl.length > 0) {
      const match = remoteUrl.match(/[:/]([^/]+\/[^/]+?)(?:\.git)?$/);
      if (match?.[1]) {
        return match[1].replaceAll("/", "-");
      }
    }
    return `${import_node_path3.default.basename(this.repoRoot)}-${this.hashRepoRoot()}`;
  }
  sanitizeBranch(branch) {
    return branch.replace(/[^A-Za-z0-9._-]/g, "-");
  }
  readRuntimeSlug(rawBranch) {
    const helperPath = resolveFromRuntimeRoot(this.runtimeRoot, "bin/superpowers-slug");
    if (pathExists(helperPath)) {
      const result = (0, import_node_child_process.spawnSync)(helperPath, [], {
        cwd: this.cwd,
        encoding: "utf8"
      });
      if (result.status === 0) {
        let slug = "";
        let safeBranch = "";
        for (const line of normalizeLineEndings(result.stdout).split("\n")) {
          if (line.startsWith("SLUG=")) {
            slug = line.slice("SLUG=".length);
          }
          if (line.startsWith("BRANCH=")) {
            safeBranch = line.slice("BRANCH=".length);
          }
        }
        if (slug.length > 0 && safeBranch.length > 0) {
          return { slug, safeBranch };
        }
      }
    }
    const remoteUrlResult = this.runGitCommand(["remote", "get-url", "origin"]);
    return {
      slug: this.deriveRepoSlug(remoteUrlResult.success ? remoteUrlResult.stdout : ""),
      safeBranch: this.sanitizeBranch(rawBranch)
    };
  }
  resetManifestFields() {
    this.manifestSpecPath = "";
    this.manifestPlanPath = "";
    this.manifestStatus = "";
    this.manifestNextSkill = "";
    this.manifestReason = "";
    this.manifestRepoRoot = "";
    this.manifestBranch = "";
    this.manifestSourcePath = "";
    this.manifestValid = false;
    this.manifestCorrupt = false;
  }
  resetManifestDiagnostics() {
    this.manifestRecovered = false;
    this.manifestRecoveryAmbiguous = false;
    this.manifestIdentityMismatch = false;
    this.manifestIdentityReason = "";
  }
  readManifestFile(filePath) {
    this.resetManifestFields();
    if (!isFilePath(filePath)) {
      return "missing";
    }
    const jsonText = readTextFileIfExists(filePath);
    if (jsonText.length === 0) {
      this.manifestCorrupt = true;
      return "corrupt";
    }
    let parsed;
    try {
      const value = JSON.parse(jsonText);
      if (value === null || Array.isArray(value) || typeof value !== "object") {
        throw new Error("invalid-manifest");
      }
      parsed = value;
    } catch {
      this.manifestCorrupt = true;
      return "corrupt";
    }
    if (parsed.version !== 1) {
      this.manifestCorrupt = true;
      return "corrupt";
    }
    this.manifestSpecPath = typeof parsed.expected_spec_path === "string" ? parsed.expected_spec_path : "";
    this.manifestPlanPath = typeof parsed.expected_plan_path === "string" ? parsed.expected_plan_path : "";
    this.manifestStatus = typeof parsed.status === "string" ? parsed.status : "";
    this.manifestNextSkill = typeof parsed.next_skill === "string" ? parsed.next_skill : "";
    this.manifestReason = typeof parsed.reason === "string" ? parsed.reason : typeof parsed.note === "string" ? parsed.note : "";
    this.manifestRepoRoot = typeof parsed.repo_root === "string" ? parsed.repo_root : "";
    this.manifestBranch = typeof parsed.branch === "string" ? parsed.branch : "";
    this.manifestSourcePath = filePath;
    this.manifestValid = true;
    return "ok";
  }
  readManifest() {
    return this.readManifestFile(this.manifestPath);
  }
  writeManifest(payload) {
    const tempPath = `${this.manifestPath}.tmp.${process.pid}-${Date.now()}`;
    ensureDirectoryExists(import_node_path3.default.dirname(this.manifestPath));
    import_node_fs2.default.writeFileSync(tempPath, `${payload}
`, "utf8");
    movePath(tempPath, this.manifestPath);
  }
  writeManifestWithRetry(payload) {
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      try {
        this.writeManifest(payload);
        return true;
      } catch {
        if (attempt === 1) {
          this.writeStderr("warning: manifest write conflict, retrying once\n");
        }
      }
    }
    return false;
  }
  backupCorruptManifest() {
    const now = /* @__PURE__ */ new Date();
    const timestamp = [
      now.getFullYear().toString().padStart(4, "0"),
      (now.getMonth() + 1).toString().padStart(2, "0"),
      now.getDate().toString().padStart(2, "0"),
      "-",
      now.getHours().toString().padStart(2, "0"),
      now.getMinutes().toString().padStart(2, "0"),
      now.getSeconds().toString().padStart(2, "0")
    ].join("");
    const backupPath = `${this.manifestPath}.corrupt-${timestamp}`;
    movePath(this.manifestPath, backupPath);
    this.writeStderr(`warning: corrupted workflow manifest moved to ${backupPath}
`);
  }
  manifestPayload(specPath, planPath, statusCode, nextSkill, reason = "") {
    const payload = {
      version: 1,
      repo_root: this.repoRoot,
      branch: this.branch,
      expected_spec_path: specPath,
      expected_plan_path: planPath,
      status: statusCode,
      next_skill: nextSkill
    };
    if (reason.length > 0) {
      payload.reason = reason;
      payload.note = reason;
    }
    payload.updated_at = (/* @__PURE__ */ new Date()).toISOString().replace(/\.\d{3}Z$/, "Z");
    return JSON.stringify(payload);
  }
  listNewestCandidates(directoryPath) {
    return listNewestFiles(directoryPath, { extension: ".md", limit: this.fallbackLimit });
  }
  relFromRoot(absolutePath) {
    if (!isPathInsideRoot(this.repoRoot, absolutePath)) {
      return null;
    }
    const relativePath = import_node_path3.default.relative(this.repoRoot, absolutePath);
    return relativePath.length > 0 ? relativePath.replaceAll(import_node_path3.default.sep, "/") : null;
  }
  repoPathExists(relativePath) {
    if (relativePath.length === 0) {
      return false;
    }
    return isFilePath(import_node_path3.default.join(this.repoRoot, relativePath));
  }
  resetResolutionFlags() {
    this.specFromManifest = false;
    this.specExpectedMissing = false;
    this.specAmbiguous = false;
    this.planFromManifest = false;
    this.planExpectedMissing = false;
    this.planAmbiguous = false;
    this.resolvedSpecPath = "";
    this.resolvedPlanPath = "";
    this.specHeaders = null;
    this.planHeaders = null;
  }
  resolvePreferredSpec() {
    if (this.manifestSpecPath.length > 0) {
      const normalized = normalizeRepoRelativePath(this.manifestSpecPath);
      if (normalized !== null) {
        this.specFromManifest = true;
        this.manifestSpecPath = normalized;
        const absolutePath = import_node_path3.default.join(this.repoRoot, normalized);
        this.resolvedSpecPath = normalized;
        if (isFilePath(absolutePath)) {
          this.specHeaders = parseSpecHeaders(readTextFileIfExists(absolutePath));
          if (this.specHeaders !== null) {
            return;
          }
        } else {
          this.specExpectedMissing = true;
        }
      } else {
        this.appendStatusNote("invalid_manifest_spec_path");
        this.manifestSpecPath = "";
      }
    }
    let firstAny = "";
    let firstValid = "";
    let firstValidHeaders = null;
    let validCount = 0;
    for (const absolutePath of this.listNewestCandidates(import_node_path3.default.join(this.repoRoot, "docs/superpowers/specs"))) {
      const localRelative = this.relFromRoot(absolutePath);
      if (localRelative === null) {
        continue;
      }
      if (this.manifestSpecPath.length > 0 && localRelative === this.manifestSpecPath) {
        continue;
      }
      if (firstAny.length === 0) {
        firstAny = localRelative;
      }
      const parsed = parseSpecHeaders(readTextFileIfExists(absolutePath));
      if (parsed !== null) {
        validCount += 1;
        if (firstValid.length === 0) {
          firstValid = localRelative;
          firstValidHeaders = parsed;
        }
      }
    }
    if (validCount > 1) {
      this.specAmbiguous = true;
      return;
    }
    if (validCount === 1 && firstValid.length > 0 && firstValidHeaders !== null) {
      this.resolvedSpecPath = firstValid;
      this.manifestSpecPath = firstValid;
      this.specHeaders = firstValidHeaders;
      return;
    }
    if (firstAny.length > 0) {
      this.resolvedSpecPath = firstAny;
      this.specHeaders = parseSpecHeaders(readTextFileIfExists(import_node_path3.default.join(this.repoRoot, firstAny)));
    }
  }
  resolvePreferredPlan() {
    if (this.manifestPlanPath.length > 0) {
      const normalized = normalizeRepoRelativePath(this.manifestPlanPath);
      if (normalized !== null) {
        this.planFromManifest = true;
        this.manifestPlanPath = normalized;
        const absolutePath = import_node_path3.default.join(this.repoRoot, normalized);
        this.resolvedPlanPath = normalized;
        if (isFilePath(absolutePath)) {
          this.planHeaders = parsePlanHeaders(readTextFileIfExists(absolutePath));
          if (this.planHeaders !== null) {
            return;
          }
        } else {
          this.planExpectedMissing = true;
        }
      } else {
        this.appendStatusNote("invalid_manifest_plan_path");
        this.manifestPlanPath = "";
      }
    }
    let firstAny = "";
    let firstValid = "";
    let firstValidHeaders = null;
    let validCount = 0;
    for (const absolutePath of this.listNewestCandidates(import_node_path3.default.join(this.repoRoot, "docs/superpowers/plans"))) {
      const localRelative = this.relFromRoot(absolutePath);
      if (localRelative === null) {
        continue;
      }
      if (this.manifestPlanPath.length > 0 && localRelative === this.manifestPlanPath) {
        continue;
      }
      if (firstAny.length === 0) {
        firstAny = localRelative;
      }
      const parsed = parsePlanHeaders(readTextFileIfExists(absolutePath));
      if (parsed !== null) {
        validCount += 1;
        if (firstValid.length === 0) {
          firstValid = localRelative;
          firstValidHeaders = parsed;
        }
      }
    }
    if (validCount > 1) {
      this.planAmbiguous = true;
      return;
    }
    if (validCount === 1 && firstValid.length > 0 && firstValidHeaders !== null) {
      this.resolvedPlanPath = firstValid;
      this.manifestPlanPath = firstValid;
      this.planHeaders = firstValidHeaders;
      return;
    }
    if (firstAny.length > 0) {
      this.resolvedPlanPath = firstAny;
      this.planHeaders = parsePlanHeaders(readTextFileIfExists(import_node_path3.default.join(this.repoRoot, firstAny)));
    }
  }
  deriveStatus() {
    this.resetResolutionFlags();
    this.statusCode = "needs_brainstorming";
    this.nextSkill = "superpowers:brainstorming";
    this.statusNotes = [];
    this.resolvePreferredSpec();
    this.resolvePreferredPlan();
    const classification = classifyWorkflowState({
      resolvedSpecPath: this.resolvedSpecPath,
      specExists: this.resolvedSpecPath.length > 0 && this.repoPathExists(this.resolvedSpecPath),
      specHeaders: this.specHeaders,
      resolvedPlanPath: this.resolvedPlanPath,
      planExists: this.resolvedPlanPath.length > 0 && this.repoPathExists(this.resolvedPlanPath),
      planHeaders: this.planHeaders,
      specAmbiguous: this.specAmbiguous,
      specExpectedMissing: this.specExpectedMissing,
      planAmbiguous: this.planAmbiguous,
      planExpectedMissing: this.planExpectedMissing
    });
    this.statusCode = classification.statusCode;
    this.nextSkill = classification.nextSkill;
    if (classification.reason.length > 0) {
      this.appendStatusNote(classification.reason);
    }
  }
  loadManifestOrRecover() {
    this.resetManifestDiagnostics();
    const currentManifest = this.readManifest();
    if (currentManifest === "ok") {
      if (this.manifestRepoRoot.length > 0 && this.manifestRepoRoot !== this.repoRoot) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = "repo_root_mismatch";
        this.writeStderr("warning: workflow manifest repo_root mismatch; rebuilding from current repo context\n");
      } else if (this.manifestBranch.length > 0 && this.manifestBranch !== this.branch) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = "branch_mismatch";
        this.writeStderr("warning: workflow manifest branch mismatch; rebuilding from current repo context\n");
      }
      return "ok";
    }
    if (currentManifest === "corrupt" || this.manifestCorrupt) {
      this.backupCorruptManifest();
      this.manifestSpecPath = "";
      this.manifestPlanPath = "";
      return "corrupt";
    }
    const candidateFileName = `${this.userName}-${this.safeBranch}-workflow-state.json`;
    const projectsRoot = import_node_path3.default.join(this.stateDir, "projects");
    let matchedCandidate = "";
    let matchedCount = 0;
    let inspected = 0;
    if (pathExists(projectsRoot)) {
      for (const entry of import_node_fs2.default.readdirSync(projectsRoot).sort((left, right) => left.localeCompare(right))) {
        const candidatePath = import_node_path3.default.join(projectsRoot, entry, candidateFileName);
        if (candidatePath === this.manifestPath || !isFilePath(candidatePath)) {
          continue;
        }
        inspected += 1;
        if (inspected > this.recoveryCandidateLimit) {
          break;
        }
        if (this.readManifestFile(candidatePath) === "ok") {
          if (this.manifestRepoRoot === this.repoRoot && this.manifestBranch === this.branch) {
            matchedCandidate = candidatePath;
            matchedCount += 1;
            if (matchedCount > 1) {
              break;
            }
          }
        }
      }
    }
    if (matchedCount > 1) {
      this.manifestRecoveryAmbiguous = true;
      this.resetManifestFields();
      this.writeStderr("warning: multiple prior workflow manifests matched this repo identity; routing conservatively\n");
      return "missing";
    }
    if (matchedCount === 1 && this.readManifestFile(matchedCandidate) === "ok") {
      this.manifestRecovered = true;
      this.writeStderr(`warning: recovered workflow manifest from previous repo slug ${matchedCandidate}
`);
      return "ok";
    }
    this.resetManifestFields();
    return "missing";
  }
  loadManifestReadOnly() {
    let currentManifestCorrupt = false;
    this.resetManifestDiagnostics();
    this.manifestCorrupt = false;
    const currentManifest = this.readManifest();
    if (currentManifest === "ok") {
      if (this.manifestRepoRoot.length > 0 && this.manifestRepoRoot !== this.repoRoot) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = "repo_root_mismatch";
      } else if (this.manifestBranch.length > 0 && this.manifestBranch !== this.branch) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = "branch_mismatch";
      }
      return "ok";
    }
    if (currentManifest === "corrupt" || this.manifestCorrupt) {
      currentManifestCorrupt = true;
      this.manifestSpecPath = "";
      this.manifestPlanPath = "";
    }
    const candidateFileName = `${this.userName}-${this.safeBranch}-workflow-state.json`;
    const projectsRoot = import_node_path3.default.join(this.stateDir, "projects");
    let matchedCandidate = "";
    let matchedCount = 0;
    let inspected = 0;
    if (pathExists(projectsRoot)) {
      for (const entry of import_node_fs2.default.readdirSync(projectsRoot).sort((left, right) => left.localeCompare(right))) {
        const candidatePath = import_node_path3.default.join(projectsRoot, entry, candidateFileName);
        if (candidatePath === this.manifestPath || !isFilePath(candidatePath)) {
          continue;
        }
        inspected += 1;
        if (inspected > this.recoveryCandidateLimit) {
          break;
        }
        if (this.readManifestFile(candidatePath) === "ok") {
          if (this.manifestRepoRoot === this.repoRoot && this.manifestBranch === this.branch) {
            matchedCandidate = candidatePath;
            matchedCount += 1;
            if (matchedCount > 1) {
              break;
            }
          }
        }
      }
    }
    if (matchedCount > 1) {
      this.manifestRecoveryAmbiguous = true;
      this.resetManifestFields();
      this.manifestCorrupt = currentManifestCorrupt;
      return "missing";
    }
    if (matchedCount === 1 && this.readManifestFile(matchedCandidate) === "ok") {
      this.manifestRecovered = true;
      this.manifestCorrupt = currentManifestCorrupt;
      return "ok";
    }
    this.resetManifestFields();
    this.manifestCorrupt = currentManifestCorrupt;
    return "missing";
  }
  manifestPathsForWrite() {
    const writeSpec = this.manifestSpecPath.length === 0 && !this.specAmbiguous ? this.resolvedSpecPath : this.manifestSpecPath;
    const writePlan = this.manifestPlanPath.length === 0 && !this.planAmbiguous ? this.resolvedPlanPath : this.manifestPlanPath;
    return [writeSpec, writePlan];
  }
  applyResolutionDiagnostics(mode, corrupted) {
    if (mode === "mutating" && corrupted) {
      this.statusCode = "needs_brainstorming";
      this.nextSkill = "superpowers:brainstorming";
      this.setStatusNotes("corrupt_manifest_recovered");
    } else if (mode === "read_only" && this.manifestCorrupt) {
      this.appendStatusNote("corrupt_manifest_present");
    }
    if (this.manifestIdentityMismatch) {
      this.appendStatusNote(this.manifestIdentityReason);
    }
    if (this.manifestRecovered) {
      this.appendStatusNote("repo_slug_recovered");
    }
    if (this.manifestRecoveryAmbiguous) {
      this.appendStatusNote("repo_slug_recovery_ambiguous");
    }
  }
  writeAndEmitResult(specPath, planPath, outputMode) {
    const payload = this.manifestPayload(specPath, planPath, this.statusCode, this.nextSkill, this.currentReason());
    if (!this.writeManifestWithRetry(payload)) {
      this.statusCode = "needs_brainstorming";
      this.nextSkill = "superpowers:brainstorming";
      this.setStatusNotes("manifest_write_conflict");
    }
    if (outputMode === "summary") {
      this.emitSummary(this.statusCode, this.nextSkill, specPath, planPath, this.currentReason());
    } else {
      this.emitStatusJson(this.statusCode, this.nextSkill, specPath, planPath, this.currentReason());
    }
    return this.result(0);
  }
  validateRepoRelativePath(input) {
    const normalized = normalizeRepoRelativePath(input);
    if (normalized === null) {
      this.writeStderr(`Invalid artifact path: ${input}
`);
      return null;
    }
    const absolutePath = import_node_path3.default.join(this.repoRoot, normalized);
    if (!isPathInsideRoot(this.repoRoot, absolutePath)) {
      this.writeStderr(`Invalid artifact path: ${input}
`);
      return null;
    }
    return normalized;
  }
  commandStatus(args) {
    let refresh = false;
    let summary = false;
    for (const arg of args) {
      if (arg === "--refresh") {
        refresh = true;
      } else if (arg === "--summary") {
        summary = true;
      } else {
        this.writeStderr(`Unknown status option: ${arg}
`);
        this.writeStdout(this.usageText());
        return this.result(1);
      }
    }
    this.ensureManifestDir();
    this.loadManifestOrRecover();
    const corrupted = this.manifestCorrupt;
    if (!refresh && this.manifestValid && !corrupted && !this.manifestRecovered && !this.manifestIdentityMismatch && !this.manifestRecoveryAmbiguous) {
      if (summary) {
        this.emitSummary(
          this.manifestStatus || "needs_brainstorming",
          this.manifestNextSkill,
          this.manifestSpecPath,
          this.manifestPlanPath,
          this.manifestReason
        );
      } else {
        this.emitStatusJson(
          this.manifestStatus || "needs_brainstorming",
          this.manifestNextSkill,
          this.manifestSpecPath,
          this.manifestPlanPath,
          this.manifestReason
        );
      }
      return this.result(0);
    }
    this.deriveStatus();
    this.applyResolutionDiagnostics("mutating", corrupted);
    const [persistedSpec, persistedPlan] = this.manifestPathsForWrite();
    return this.writeAndEmitResult(persistedSpec, persistedPlan, summary ? "summary" : "json");
  }
  commandResolve(args) {
    for (const arg of args) {
      if (arg !== "--debug") {
        this.emitResolveFailure("InvalidCommandInput", `Unknown resolve option: ${arg}`);
        return this.result(1);
      }
    }
    switch (this.env.SUPERPOWERS_WORKFLOW_RESOLVE_TEST_FAILPOINT) {
      case "runtime_failure":
        this.emitResolveFailure("ResolverRuntimeFailure", "Injected read-only resolver failpoint triggered.");
        return this.result(1);
      case "invalid_contract":
        this.emitResolveFailure("ResolverContractViolation", "Injected invalid resolve-contract failpoint triggered.");
        return this.result(1);
      default:
        break;
    }
    if (this.env.SUPERPOWERS_WORKFLOW_STATUS_TEST_FAILPOINT === "resolve_runtime_failure") {
      this.emitResolveFailure("ResolverRuntimeFailure", "Injected read-only resolver failpoint triggered.");
      return this.result(1);
    }
    if (!this.repoContextAvailable) {
      this.emitResolveFailure("RepoContextUnavailable", "Read-only workflow resolution requires a git repo.");
      return this.result(1);
    }
    this.loadManifestReadOnly();
    this.deriveStatus();
    this.applyResolutionDiagnostics("read_only", false);
    this.emitResolveJson(this.statusCode, this.nextSkill, this.resolvedSpecPath, this.resolvedPlanPath, this.currentReason());
    return this.result(0);
  }
  commandExpect(args) {
    let artifact = "";
    let artifactPath = "";
    for (let index = 0; index < args.length; index += 1) {
      const arg = args[index];
      if (arg === "--artifact") {
        artifact = args[index + 1] ?? "";
        index += 1;
      } else if (arg === "--path") {
        artifactPath = args[index + 1] ?? "";
        index += 1;
      } else {
        this.writeStderr(`Unknown expect option: ${arg}
`);
        this.writeStdout(this.usageText());
        return this.result(1);
      }
    }
    if (artifact !== "spec" && artifact !== "plan") {
      this.writeStderr("expect requires --artifact {spec|plan}\n");
      return this.result(1);
    }
    if (artifactPath.length === 0) {
      this.writeStderr("expect requires --path <repo-relative-path>\n");
      return this.result(1);
    }
    this.ensureManifestDir();
    this.loadManifestOrRecover();
    const normalized = this.validateRepoRelativePath(artifactPath);
    if (normalized === null) {
      return this.result(1);
    }
    if (artifact === "spec") {
      this.manifestSpecPath = normalized;
    } else {
      this.manifestPlanPath = normalized;
    }
    this.deriveStatus();
    this.appendStatusNote("expect_set");
    const [persistedSpec, persistedPlan] = this.manifestPathsForWrite();
    return this.writeAndEmitResult(persistedSpec, persistedPlan, "json");
  }
  commandSync(args) {
    let artifact = "";
    let artifactPath = "";
    for (let index = 0; index < args.length; index += 1) {
      const arg = args[index];
      if (arg === "--artifact") {
        artifact = args[index + 1] ?? "";
        index += 1;
      } else if (arg === "--path") {
        artifactPath = args[index + 1] ?? "";
        index += 1;
      } else {
        this.writeStderr(`Unknown sync option: ${arg}
`);
        this.writeStdout(this.usageText());
        return this.result(1);
      }
    }
    if (artifact !== "spec" && artifact !== "plan") {
      this.writeStderr("sync requires --artifact {spec|plan}\n");
      return this.result(1);
    }
    this.ensureManifestDir();
    this.loadManifestOrRecover();
    let targetPath = "";
    if (artifactPath.length > 0) {
      const normalized = this.validateRepoRelativePath(artifactPath);
      if (normalized === null) {
        return this.result(1);
      }
      targetPath = normalized;
    } else {
      targetPath = artifact === "spec" ? this.manifestSpecPath : this.manifestPlanPath;
    }
    const missingArtifact = targetPath.length === 0 || !this.repoPathExists(targetPath);
    if (artifact === "spec") {
      this.manifestSpecPath = targetPath;
    } else {
      this.manifestPlanPath = targetPath;
    }
    this.deriveStatus();
    this.appendStatusNote(`sync_${artifact}`);
    if (missingArtifact) {
      this.appendStatusNote("missing_artifact");
      if (artifact === "spec") {
        this.statusCode = "needs_brainstorming";
        this.nextSkill = "superpowers:brainstorming";
      }
    }
    const [persistedSpec, persistedPlan] = this.manifestPathsForWrite();
    return this.writeAndEmitResult(persistedSpec, persistedPlan, "json");
  }
};
function runWorkflowStatusCommand(args, options = {}) {
  return new WorkflowStatusRunner(options).run(args);
}

// src/platform/process.ts
function runCli(main2, argv = process.argv) {
  process.exitCode = main2(argv);
}

// src/cli/superpowers-workflow-status.ts
function main(argv = process.argv) {
  const entryPath = argv[1] ?? import_node_path4.default.join(process.cwd(), "runtime/core-helpers/dist/superpowers-workflow-status.cjs");
  const result = runWorkflowStatusCommand(argv.slice(2), {
    cwd: process.cwd(),
    env: process.env,
    runtimeRoot: resolveRuntimeRoot(entryPath, process.env.SUPERPOWERS_RUNTIME_ROOT)
  });
  if (result.stdout.length > 0) {
    process.stdout.write(result.stdout);
  }
  if (result.stderr.length > 0) {
    process.stderr.write(result.stderr);
  }
  return result.exitCode;
}
if (typeof require !== "undefined" && require.main === module) {
  runCli(main);
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  main
});
