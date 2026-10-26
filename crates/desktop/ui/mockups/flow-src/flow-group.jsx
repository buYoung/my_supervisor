import React, {memo} from 'react';

export const FlowGroup = memo(function FlowGroup({data}) {
  return <div className="flow-auto-group">
    <header className="flow-group-header" title="드래그하여 그룹 전체 이동">
      <span className="group-grip" aria-hidden="true">⠿</span>
      <input className="nodrag nowheel" aria-label="그룹 이름" value={data.name} disabled={data.disabled} maxLength={80} onChange={event=>data.onRename(event.target.value)} onKeyDown={event=>event.stopPropagation()}/>
      <span>{data.count}개 {data.unit}</span>
    </header>
  </div>;
});
