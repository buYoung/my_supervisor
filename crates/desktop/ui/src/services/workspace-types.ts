export type StepKind = "trigger"|"command"|"condition"|"wait"|"process"|"health"|"job"|"parallel"|"join"|"finish";
export interface ResultRoute {id:string;label:string;source:string;operator:string;value:string}
export interface StepData extends Record<string,unknown> {kind:StepKind;label:string;command?:string;program?:string;args?:string[];cwd?:string;environment?:Record<string,string>;resultRoutes?:ResultRoute[];processName?:string;operation?:string;seconds?:number;source?:string;operator?:string;value?:string;path?:string;checkType?:string;url?:string;host?:string;port?:number;expectedStatus?:number;jobId?:string;result?:string;message?:string;joinMode?:string;failurePolicy?:string;maxAttempts?:number;timeoutSeconds?:number;retryStrategy?:string;retryDelaySeconds?:number;retryIncrementSeconds?:number;retryMultiplier?:number;retryMaxDelaySeconds?:number;retryJitter?:boolean;retryOn?:string[];onError?:string;circuitBreaker?:{enabled:boolean;failureThreshold:number;openSeconds:number;recoverySuccesses:number}}
export interface FlowNode {id:string;position:{x:number;y:number};data:StepData}
export interface FlowEdge {id:string;source:string;target:string;sourceHandle:string}
export interface Workflow {version:number;nodes:FlowNode[];edges:FlowEdge[];group?:{name:string;members:string[];position?:{x:number;y:number}}|null;settings?:Record<string,unknown>}
export interface WorkflowDocument {revision:number;saved_at:string|null;definition:Workflow}
export interface WorkflowHistoryEntry {id:string;version:number|null;state:string;startedAt?:string;finishedAt?:string;hasSnapshot:boolean;path:Array<{id:string;label:string;state:string}>}
export interface StepResult {id:string;state:string;attempt?:number;startedAt?:string;finishedAt?:string;exitCode?:number;selectedPort?:string;stdout?:string;stderr?:string;summary?:string;logs:Array<{time:string;stream:string;message:string}>}
export interface WorkflowRun {id:string;jobId:string;version:number;state:string;startedAt:string;finishedAt?:string;durationMs?:number;exitCode?:number;workflow:Workflow|null;steps:Record<string,StepResult>;edges:string[];summary?:string}
export interface Executable {name:string;path:string;version:string;is_default:boolean}
export interface CommandHelp {program:string;text:string;supported:boolean;cached:boolean}
export interface RuntimeRelease {version:string;label:string;url:string;sha256:string}
