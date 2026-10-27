"""Read-only esptool 4.x backup with one acknowledged packet in flight.

Uses the documented READ_FLASH window field, without copying esptool code.
Protocol: https://docs.espressif.com/projects/esptool/en/release-v4/esp32/advanced-topics/serial-protocol.html
"""
import argparse, struct, time, json
from pathlib import Path
import esptool, serial
from esptool.loader import ESPLoader
from esptool.reset import HardReset

parser=argparse.ArgumentParser()
parser.add_argument('--port',default='COM3')
parser.add_argument('--baud',type=int,default=115200)
parser.add_argument('--size',type=lambda s:int(s,0),required=True)
parser.add_argument('--output',required=True)
a=parser.parse_args()
if Path(a.output).exists():parser.error('Backup output already exists; choose a new file')
if not Path(a.output).parent.is_dir():parser.error('Backup output parent must exist')
original=ESPLoader.check_command
original_read=ESPLoader.read_flash
ports=[]

def one_packet(self,description,op=None,data=b'',*args,**kwargs):
    if self._port not in ports: ports.append(self._port)
    if op==self.ESP_READ_FLASH and len(data)==16:
        offset,length,block,window=struct.unpack('<IIII',data)
        data=struct.pack('<IIII',offset,length,block,1)
    return original(self,description,op,data,*args,**kwargs)

def read_with_progress(self,offset,length,progress_fn=None):
    last=[0]
    def progress(done,total):
        if done==total or done-last[0]>=262144:
            print(json.dumps({'read_bytes':done,'total_bytes':total}),flush=True)
            last[0]=done
        if progress_fn:progress_fn(done,total)
    return original_read(self,offset,length,progress)

ESPLoader.read_flash=read_with_progress
ESPLoader.check_command=one_packet
success=False
try:
    esptool.main(['--chip','esp32s3','--port',a.port,'--baud',str(a.baud),'--after','hard_reset',
                  'read_flash','--no-progress','0',hex(a.size),a.output])
    success=True
finally:
    ESPLoader.check_command=original
    ESPLoader.read_flash=original_read
    for opened in ports:
        if opened.is_open: opened.close()
    if not success:
        # Never leave the installed application in the loader after an error.
        time.sleep(.2)
        p=serial.Serial();p.port=a.port;p.baudrate=115200;p.timeout=.2;p.write_timeout=1;p.dtr=False;p.rts=False
        try:
            p.open();HardReset(p).reset()
        finally:
            if p.is_open:p.close()
