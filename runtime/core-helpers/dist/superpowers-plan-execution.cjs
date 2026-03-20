"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
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
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/cli/superpowers-plan-execution.ts
var superpowers_plan_execution_exports = {};
__export(superpowers_plan_execution_exports, {
  main: () => main
});
module.exports = __toCommonJS(superpowers_plan_execution_exports);

// src/platform/process.ts
function runCli(main2, argv = process.argv) {
  process.exitCode = main2(argv);
}

// src/cli/superpowers-plan-execution.ts
function main() {
  console.error("Not implemented: superpowers-plan-execution");
  return 1;
}
if (typeof require !== "undefined" && require.main === module) {
  runCli(() => main());
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  main
});
