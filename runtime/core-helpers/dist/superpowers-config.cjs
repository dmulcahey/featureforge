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

// src/cli/superpowers-config.ts
var superpowers_config_exports = {};
__export(superpowers_config_exports, {
  main: () => main
});
module.exports = __toCommonJS(superpowers_config_exports);
var import_node_path3 = __toESM(require("node:path"), 1);

// src/core/config.ts
var KEY_VALUE_PATTERN = /^([^:]+):\s*(.*)$/;
function normalizeConfigText(configText) {
  if (configText.length === 0) {
    return [];
  }
  return configText.replace(/\r\n/g, "\n").split("\n").filter((line, index, lines) => !(index === lines.length - 1 && line === ""));
}
function getConfigValue(configText, key) {
  let matchedValue = "";
  for (const line of normalizeConfigText(configText)) {
    const match = line.match(KEY_VALUE_PATTERN);
    if (match && match[1] === key) {
      matchedValue = match[2].replace(/\s+/g, "");
    }
  }
  return matchedValue;
}
function setConfigValue(configText, key, value) {
  const normalizedLines = normalizeConfigText(configText);
  const updatedLines = [];
  let updated = false;
  for (const line of normalizedLines) {
    const match = line.match(KEY_VALUE_PATTERN);
    if (match && match[1] === key) {
      updatedLines.push(`${key}: ${value}`);
      updated = true;
      continue;
    }
    updatedLines.push(line);
  }
  if (!updated) {
    updatedLines.push(`${key}: ${value}`);
  }
  return updatedLines.length > 0 ? `${updatedLines.join("\n")}
` : "";
}

// src/platform/filesystem.ts
var import_node_fs = __toESM(require("node:fs"), 1);
var import_node_path = __toESM(require("node:path"), 1);
function pathExists(filePath) {
  return import_node_fs.default.existsSync(filePath);
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
function writeTextFileAtomic(filePath, contents) {
  ensureDirectoryExists(import_node_path.default.dirname(filePath));
  const tempPath = `${filePath}.tmp-${process.pid}-${Date.now()}`;
  import_node_fs.default.writeFileSync(tempPath, contents, "utf8");
  import_node_fs.default.renameSync(tempPath, filePath);
}

// src/platform/paths.ts
var import_node_os = __toESM(require("node:os"), 1);
var import_node_path2 = __toESM(require("node:path"), 1);
function resolveStateDir(env, platform = process.platform) {
  const pathApi = platform === "win32" ? import_node_path2.default.win32 : import_node_path2.default;
  const bashStyleHomeMatch = platform === "win32" ? env.HOME?.match(/^\/([A-Za-z])(?:\/(.*))?$/) : null;
  const uncStyleHomeMatch = platform === "win32" ? env.HOME?.match(/^\/\/([^/]+)\/([^/]+)(?:\/(.*))?$/) : null;
  if (env.SUPERPOWERS_STATE_DIR && env.SUPERPOWERS_STATE_DIR.length > 0) {
    return env.SUPERPOWERS_STATE_DIR;
  }
  if (platform === "win32") {
    if (env.USERPROFILE && env.USERPROFILE.length > 0) {
      return pathApi.join(env.USERPROFILE, ".superpowers");
    }
    if (env.HOMEDRIVE && env.HOMEPATH && env.HOMEDRIVE.length > 0 && env.HOMEPATH.length > 0) {
      return pathApi.join(`${env.HOMEDRIVE}${env.HOMEPATH}`, ".superpowers");
    }
    if (bashStyleHomeMatch) {
      const drive = `${bashStyleHomeMatch[1].toUpperCase()}:\\`;
      const rest = bashStyleHomeMatch[2] ? bashStyleHomeMatch[2].replace(/\//g, "\\") : "";
      return pathApi.join(drive, rest, ".superpowers");
    }
    if (uncStyleHomeMatch) {
      const server = uncStyleHomeMatch[1];
      const share = uncStyleHomeMatch[2];
      const rest = uncStyleHomeMatch[3] ? uncStyleHomeMatch[3].replace(/\//g, "\\") : "";
      return pathApi.join(`\\\\${server}\\${share}`, rest, ".superpowers");
    }
    if (env.HOME && env.HOME.length > 0) {
      return pathApi.join(env.HOME, ".superpowers");
    }
    return pathApi.join(import_node_os.default.homedir(), ".superpowers");
  }
  if (env.HOME && env.HOME.length > 0) {
    return pathApi.join(env.HOME, ".superpowers");
  }
  return pathApi.join(import_node_os.default.homedir(), ".superpowers");
}

// src/platform/process.ts
function runCli(main2, argv = process.argv) {
  process.exitCode = main2(argv);
}

// src/cli/superpowers-config.ts
var USAGE = "Usage: superpowers-config {get|set|list} [key] [value]";
function resolveConfigFile() {
  const stateDir = resolveStateDir(process.env);
  return import_node_path3.default.join(stateDir, "config.yaml");
}
function writeUsage() {
  console.error(USAGE);
  return 1;
}
function main(argv = process.argv) {
  const [, , command, key, value] = argv;
  const configFile = resolveConfigFile();
  switch (command) {
    case "get": {
      if (!key) {
        return writeUsage();
      }
      const resolvedValue = getConfigValue(readTextFileIfExists(configFile), key);
      if (resolvedValue) {
        process.stdout.write(`${resolvedValue}
`);
      }
      return 0;
    }
    case "set": {
      if (!key || value === void 0) {
        return writeUsage();
      }
      const updatedConfig = setConfigValue(readTextFileIfExists(configFile), key, value);
      writeTextFileAtomic(configFile, updatedConfig);
      return 0;
    }
    case "list": {
      const configText = readTextFileIfExists(configFile);
      if (configText) {
        process.stdout.write(configText);
      }
      return 0;
    }
    default:
      return writeUsage();
  }
}
if (typeof require !== "undefined" && require.main === module) {
  runCli((argv) => main(argv));
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  main
});
