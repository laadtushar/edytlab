import subprocess, json, sys
W,H=160,9
caps=json.load(open('captions-final.json'))
out={}
for slug,info in caps.items():
    src=f'/home/user/edytlab/website/public/demos/{slug}.mp4'
    p=subprocess.run(['nice','-n','10','ffmpeg','-nostdin','-v','error','-i',src,'-vf',f'crop=1280:72:0:688,scale={W}:{H}:flags=area,format=gray,fps=15','-f','rawvideo','-'],capture_output=True)
    data=p.stdout; fs=W*H; n=len(data)//fs
    diffs=[0]
    for i in range(1,n):
        a=data[(i-1)*fs:i*fs]; b=data[i*fs:(i+1)*fs]
        diffs.append(sum(abs(x-y) for x,y in zip(a,b)))
    print(slug,'frames',n,'dur',n/15)
    res=[]
    for c in info['caps']:
        t=c['final']
        lo=max(1,int((t-4)*15)); hi=min(n-1,int((t+4)*15))
        best=max(range(lo,hi+1),key=lambda i:diffs[i])
        res.append((c['text'],t,best/15,diffs[best]))
        print(f"  computed {t:7.2f}  detected {best/15:7.2f}  delta {best/15-t:+.2f}  diff={diffs[best]:6d}  {c['text'][:50]}")
    out[slug]=res
