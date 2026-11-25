import {BenchmarkRecorder} from '@buoy-gg/perf-monitor/web';
let active=false,lesson=false;
const visible=id=>Array.from(document.querySelectorAll('[data-testid="'+id+'"]')).some(el=>el.checkVisibility() && el.getBoundingClientRect().width>0);
document.addEventListener('click',e=>{
 if (!active && e.target.closest('[data-testid="TamilCollection-first-steps"]')) {
    BenchmarkRecorder.start({name:'Tamil library journey',source:'manual'}, {captureRenders:false});
    active=true;
 }
},true);
setInterval(()=>{
 if (!active) return;
 if (visible('TamilStartPractice')) lesson=true;
 if (lesson && visible('TamilCollection-first-steps') && !visible('TamilPractice') && !visible('TamilStartPractice')) {
    active=false;lesson=false;
    const report=BenchmarkRecorder.stop();
    void fetch('/benchmark-result',{method:'POST',body:JSON.stringify(report),keepalive:true});
 }
},50);
