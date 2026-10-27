/** Local snapshots only; restoring one never executes or reverses a job. */
export class DraftHistory<T> {
  private past:T[]=[];
  private future:T[]=[];
  get canUndo(){return this.past.length>0;}
  get canRedo(){return this.future.length>0;}
  record(value:T){this.past.push(structuredClone(value));if(this.past.length>100)this.past.shift();this.future=[];}
  undo(current:T){const previous=this.past.pop();if(previous===undefined)return undefined;this.future.push(structuredClone(current));return previous;}
  redo(current:T){const next=this.future.pop();if(next===undefined)return undefined;this.past.push(structuredClone(current));return next;}
  clear(){this.past=[];this.future=[];}
}
