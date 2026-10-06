from pathlib import Path
import json
p=Path('/var/tmp/hm-private-receiving-capacity-kvm-v3/private-capacity-traffic.json');t=json.loads(p.read_text())
def parse(text):
 lines=text.splitlines();udp=[line.split()[1:] for line in lines if line.startswith('Udp:')];assert len(udp)==2
 counters=dict(zip(udp[0],map(int,udp[1])))
 sockets=[]
 for line in lines:
  fields=line.split()
  if len(fields)>12 and fields[1].endswith(':46A2'):sockets.append({'local':fields[1],'remote':fields[2],'inode':fields[9],'drops':int(fields[-1])})
 return {'counters':counters,'target_echo_sockets':sockets}
before=parse(t['guest_udp_counters_before']);after=parse(t['guest_udp_counters_after']);out={'before':before,'after':after,'udp_counter_delta':{key:after['counters'][key]-value for key,value in before['counters'].items()},'completed_workers':sum(row['success'] for row in t['rows']),'failures':[row for row in t['rows'] if not row['success']]}
print(json.dumps(out,indent=2));Path('/var/tmp/hm-private-capacity-drop-analysis-v1.json').write_text(json.dumps(out,indent=2)+'\n')
