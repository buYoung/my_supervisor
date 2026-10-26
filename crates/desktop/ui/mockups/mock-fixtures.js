/* Deterministic design fixtures. These are not readings from a running daemon. */
(() => {
  const snapshot = '2026-10-26T10:00:00+09:00';
  const profiles = {
    'api-server': {
      runtime: 'Node.js 22.14.0', release: 'v2.4.1', command: 'node --enable-source-maps dist/server.js', cwd: '/Users/demo/services/api',
      environment: { NODE_ENV: 'production', PORT: '3000', APP_VERSION: '2.4.1' },
      startedAt: '2026.10.23 09:42:18', registeredAt: '2026.04.01 09:00:00', autostart: true, lifecycle: '런타임과 함께 종료',
      restart: '자동 재시작 · 최대 10회', backoff: '1초 → 최대 60초 · 지수 증가', stableAfter: '60초', shutdown: 'SIGTERM → 10초 대기',
      rolling: {routability: true, readinessTimeoutSeconds: 30},
      memoryLimitMiB: 512, cpuHistory: [1.1,1.4,1.2,2.1,1.7,1.6,3.7,2.4,1.8,2.0,1.5,1.9,1.6,1.7,1.8],
      memoryHistory: [132,135,134,140,142,141,151,148,145,143,145,146,145,147,146.8],
      guards: [
        {label:'생존 확인',state:'healthy',summary:'HTTP 200 · /health',detail:'5초 간격 · 제한 시간 2초'},
        {label:'준비 상태',state:'healthy',summary:'HTTP 200 · /ready',detail:'연속 2회 성공'},
        {label:'메모리 제한',state:'healthy',summary:'146.8 / 512 MiB',detail:'3회 연속 초과 시 재시작'},
        {label:'파일 감시',state:'disabled',summary:'사용 안 함',detail:'운영 환경'}
      ],
      events: [
        {date:'2026-10-26',time:'09:42:18',title:'준비 상태 정상',detail:'모든 인스턴스에서 응답 확인',tone:'green'},
        {date:'2026-10-23',time:'09:42:18',title:'수동 재시작 완료',detail:'v2.4.1 · 인스턴스 2개 실행',tone:'green'},
        {date:'2026-07-15',time:'14:12:09',title:'메모리 제한으로 재시작',detail:'512 MiB 초과 · 자동 복구',tone:'amber'}
      ]
    },
    'worker-queue': {
      runtime: 'Node.js 22.14.0', release: 'v1.8.3', command: 'node dist/worker.js --queue=default', cwd: '/Users/demo/services/worker',
      environment: { NODE_ENV: 'production', QUEUE_NAME: 'default', CONCURRENCY: '4' },
      startedAt: '2026.10.25 22:41:05', registeredAt: '2026.04.01 09:10:00', autostart: true, lifecycle: '독립 실행',
      restart: 'macOS 서비스에서 관리', backoff: '시스템 정책에 따름', stableAfter: '—', shutdown: 'SIGTERM → 10초 대기',
      memoryLimitMiB: 256, cpuHistory: [.3,.2,.8,1.2,.6,.4,2.1,1.5,.6,.8,.4,.7,.9,.6,.7],
      memoryHistory: [78,80,79,85,86,88,90,96,95,94,91,93,90,92,92.3],
      guards: [
        {label:'생존 확인',state:'healthy',summary:'TCP 127.0.0.1:3100',detail:'10초 간격 · 제한 시간 2초'},
        {label:'준비 상태',state:'healthy',summary:'작업 수신 가능',detail:'연속 2회 성공'},
        {label:'메모리 제한',state:'healthy',summary:'92.3 / 256 MiB',detail:'3회 연속 초과 시 재시작'},
        {label:'파일 감시',state:'disabled',summary:'사용 안 함',detail:'운영 환경'}
      ],
      events: [
        {date:'2026-10-26',time:'09:58:04',title:'작업 처리 완료',detail:'queue=default · job=mail-1028',tone:'green'},
        {date:'2026-10-25',time:'22:41:05',title:'프로세스 복구',detail:'재시작 누적 2회',tone:'green'},
        {date:'2026-10-25',time:'22:40:59',title:'생존 확인 실패',detail:'연결 실패 후 시스템에서 재시작',tone:'amber'}
      ]
    },
    'backup-agent': {
      runtime: '실행 파일', release: 'v1.2.0', command: '/usr/local/bin/backup-agent --config ./backup.toml', cwd: '/Users/demo/services/backup',
      environment: { BACKUP_TARGET: '/Volumes/Backup', RETENTION_DAYS: '30' },
      startedAt: '—', registeredAt: '2026.04.01 09:20:00', autostart: true, lifecycle: '독립 실행',
      restart: 'macOS 서비스에서 관리', backoff: '시스템 정책에 따름', stableAfter: '—', shutdown: 'SIGTERM → 30초 대기',
      memoryLimitMiB: 128, cpuHistory: [], memoryHistory: [],
      guards: [
        {label:'생존 확인',state:'unhealthy',summary:'프로세스 없음',detail:'실행 단계에서 실패'},
        {label:'준비 상태',state:'unknown',summary:'확인할 수 없음',detail:'프로세스 실행 대기'},
        {label:'메모리 제한',state:'unknown',summary:'128 MiB',detail:'현재 측정값 없음'},
        {label:'파일 감시',state:'disabled',summary:'사용 안 함',detail:'운영 환경'}
      ],
      events: [
        {date:'2026-10-26',time:'09:58:21',title:'실행 실패 · ENOENT',detail:'/usr/local/bin/backup-agent 파일 없음',tone:'red'},
        {date:'2026-10-26',time:'09:58:20',title:'시스템 서비스 시작 요청',detail:'my-supervisor-managed-backup-agent',tone:'amber'},
        {date:'2026-10-25',time:'21:00:14',title:'백업 완료',detail:'1,284개 파일 · 814 MiB',tone:'green'}
      ]
    }
  };
  const processes = [
    { name:'api-server', state:'running', label:'실행 중', mode:'direct', modeLabel:'직접 관리', pid:'48216', cpu:'1.8', memory:'146.8', uptime:'3일 18분', restarts:1, instances:2 },
    { name:'worker-queue', state:'running', label:'실행 중', mode:'system_registered', modeLabel:'시스템 등록', pid:'48231', cpu:'0.7', memory:'92.3', uptime:'11시간 18분', restarts:2, instances:1 },
    { name:'backup-agent', state:'crashed', label:'비정상 종료', mode:'system_registered', modeLabel:'시스템 등록', pid:'—', cpu:'—', memory:'—', uptime:'—', restarts:5, instances:1 }
  ];
  const logs = [];
  const endTime = Date.parse(snapshot);
  const startDay = Date.UTC(2026,3,1);
  const endDay = Date.UTC(2026,9,26);
  const addLog = (source,date,hour,minute,second,stream,message,level='info') => {
    const timestamp = Date.parse(`${date}T${String(hour).padStart(2,'0')}:${String(minute).padStart(2,'0')}:${String(second).padStart(2,'0')}+09:00`);
    if(timestamp>endTime) return;
    const localTime = `${String(hour).padStart(2,'0')}:${String(minute).padStart(2,'0')}:${String(second).padStart(2,'0')}`;
    logs.push({ id:`${source}-${timestamp}-${logs.length}`, source, timestamp, date, month:date.slice(0,7), localTime, stream, message, level });
  };
  for(let day=startDay,dayIndex=0;day<=endDay;day+=86400000,dayIndex++) {
    const date=new Date(day).toISOString().slice(0,10);
    for(let hour=0;hour<24;hour++) {
      const minute=(hour*7+dayIndex*3)%60;
      const hasError=dayIndex%13===0 && hour===14;
      addLog('api-server',date,hour,minute,12,hasError?'stderr':'stdout',hasError?`Redis timeout after 500ms · request_id=req-${dayIndex}-${hour}`:`GET ${['/health','/api/orders','/api/users','/ready'][hour%4]} 200 · ${3+(dayIndex+hour)%38}ms · request_id=req-${dayIndex}-${hour}`,hasError?'error':'info');
      if(hour%6===0) addLog('api-server',date,hour,minute,15,'system',`resource.sample cpu=${(0.4+(dayIndex+hour)%23/10).toFixed(1)}% memory=${130+(dayIndex+hour)%26}MiB instances=2`,'debug');
    }
    for(let hour=0;hour<24;hour+=2) {
      const hasError=dayIndex%17===0 && hour===20;
      addLog('worker-queue',date,hour,11,4,hasError?'stderr':'stdout',hasError?`job.retry queue=default attempt=2 reason=connection_reset`:`job.completed queue=default id=job-${dayIndex}-${hour} duration=${80+(dayIndex+hour)%430}ms`,hasError?'warn':'info');
    }
    for(const hour of [9,21]) addLog('backup-agent',date,hour,0,14,'stdout',`backup.completed files=${1100+dayIndex%240} size=${720+dayIndex%160}MiB retention=30d`);
  }
  addLog('api-server','2026-07-15',14,12,9,'system','guard.restart reason=memory_ceiling observed=528MiB limit=512MiB','warn');
  addLog('api-server','2026-10-23',9,42,18,'system','process.restart completed release=v2.4.1 instances=2');
  addLog('api-server','2026-10-26',9,59,2,'stdout','GET /health 200 · 2ms · instance=0');
  addLog('api-server','2026-10-26',9,59,4,'stdout','GET /ready 200 · 3ms · instance=1');
  addLog('worker-queue','2026-10-25',22,40,59,'stderr','liveness.failed connection refused 127.0.0.1:3100','error');
  addLog('worker-queue','2026-10-25',22,41,5,'system','process.running pid=48231 restart_count=2');
  addLog('worker-queue','2026-10-26',9,58,4,'stdout','job.completed queue=default id=mail-1028 duration=184ms');
  addLog('backup-agent','2026-10-26',9,58,20,'system','process.start requested unit=my-supervisor-managed-backup-agent');
  addLog('backup-agent','2026-10-26',9,58,21,'stderr','spawn_failed: ENOENT · binary not found at /usr/local/bin/backup-agent','fatal');
  for(const second of [28,35,49])addLog('backup-agent','2026-10-26',9,58,second,'stderr','spawn_failed: ENOENT · binary not found at /usr/local/bin/backup-agent','fatal');
  addLog('backup-agent','2026-10-26',9,58,22,'system','process.state starting -> crashed · restart_count=5','error');
  addLog('api-server','2026-10-26',9,54,10,'stdout','{"level":"warn","message":"Redis 응답 지연 · 재시도 예약","request_id":"req-1026-92","retry_after_ms":500}','warn');
  addLog('api-server','2026-10-26',9,54,11,'stderr','[INFO] 연결 풀을 다시 구성하고 있습니다.','info');
  addLog('api-server','2026-10-26',9,54,12,'stdout','{"level":"info","message":"Redis 재연결 완료","request_id":"req-1026-92"}');
  addLog('api-server','2026-10-26',9,54,13,'stdout','[TRACE] redis.handshake duration=3ms','trace');
  addLog('api-server','2026-10-26',9,55,1,'stdout','레벨 표기가 없는 외부 모듈의 원본 출력',null);
  logs.sort((left,right)=>left.timestamp-right.timestamp);
  window.supervisorMockFixtures = { icons:{"Folder": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-folder\" aria-hidden=\"true\" focusable=\"false\"><path d=\"M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z\"></path></svg>", "FolderOpen": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-folder-open\" aria-hidden=\"true\" focusable=\"false\"><path d=\"m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2\"></path></svg>", "FileDiff": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-file-diff\" aria-hidden=\"true\" focusable=\"false\"><path d=\"M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z\"></path><path d=\"M9 10h6\"></path><path d=\"M12 13V7\"></path><path d=\"M9 17h6\"></path></svg>", "GitMerge": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-git-merge\" aria-hidden=\"true\" focusable=\"false\"><circle cx=\"18\" cy=\"18\" r=\"3\"></circle><circle cx=\"6\" cy=\"6\" r=\"3\"></circle><path d=\"M6 21V9a9 9 0 0 0 9 9\"></path></svg>", "Upload": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-upload\" aria-hidden=\"true\" focusable=\"false\"><path d=\"M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4\"></path><polyline points=\"17 8 12 3 7 8\"></polyline><line x1=\"12\" x2=\"12\" y1=\"3\" y2=\"15\"></line></svg>", "CheckCheck": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-check-check\" aria-hidden=\"true\" focusable=\"false\"><path d=\"M18 6 7 17l-5-5\"></path><path d=\"m22 10-7.5 7.5L13 16\"></path></svg>", "ArrowUp": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-arrow-up\" aria-hidden=\"true\" focusable=\"false\"><path d=\"m5 12 7-7 7 7\"></path><path d=\"M12 19V5\"></path></svg>", "Undo2": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"lucide lucide-undo2\" aria-hidden=\"true\" focusable=\"false\"><path d=\"M9 14 4 9l5-5\"></path><path d=\"M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11\"></path></svg>"}, revision:'operations-history-v1', snapshot, firstDate:'2026-04-01', lastDate:'2026-10-26', profiles, processes, logs };
})();
