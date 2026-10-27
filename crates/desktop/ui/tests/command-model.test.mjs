import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import ts from "typescript";

const source=readFileSync(new URL("../src/features/processes/command-model.ts",import.meta.url),"utf8");
const output=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {argumentRows,rowsToArguments,splitCommand,quoteArgument}=await import("data:text/javascript;base64,"+Buffer.from(output).toString("base64"));
const cases=[
  ["run","start","--","--port","3000"], [""], ["native-recorder","x"],
  ["--empty=","--tag","a","--tag","b"], ["값,alpha","한 글","quote'\"","\\path"],
  ["-Dname=","-Xmx2048m","file.jar","--","-1"], ["--port","",""],
];
for(const program of ["pnpm","node","java","/tmp/native-recorder"])for(const args of cases){
  assert.deepEqual(rowsToArguments(argumentRows(program,args)),args,`${program}: ${JSON.stringify(args)}`);
  assert.deepEqual(splitCommand(args.map(quoteArgument).join(" ")),args);
}
const rows=argumentRows("pnpm",["run","start","--","--port","3000"]);
rows.at(-1).value="4000";
assert.deepEqual(rowsToArguments(rows),["run","start","--","--port","4000"]);
rows[0].scope="runtime";
assert.deepEqual(rowsToArguments(rows),["run","start","--","--port","4000"]);
for(const line of ["'unterminated",'"unterminated',"trailing\\"])assert.throws(()=>splitCommand(line));
console.log("command model: 28 exact round trips, ordered edit, scope edit and invalid syntax passed");
