import subprocess, sys, re, struct
def rms(demo, a, b, step=0.5):
    p = subprocess.run(["nice","-n","10","ffmpeg","-nostdin","-v","error","-ss",str(a),"-t",str(b-a),"-i",f"/home/user/edytlab/website/public/demos/{demo}.mp4","-vn","-ac","1","-ar","16000","-f","s16le","-"],capture_output=True)
    d = p.stdout; n = len(d)//2
    s = struct.unpack(f"<{n}h", d)
    w = int(16000*step); out = []
    import math
    for i in range(0, n-w+1, w):
        seg = s[i:i+w]; e = (sum(x*x for x in seg)/w) ** 0.5
        out.append(20*math.log10(max(e,1)/32768))
    return out
for demo,a,b in [("dj-beatmatched-transition",91,110),("dj-extended-club-intro",85,94),("dj-extended-club-intro",98,111),("dj-mini-mix",90,104),("dj-mini-mix",108,122),("dj-beatmatched-transition",170,183)]:
    r = rms(demo,a,b)
    print(demo, a, b, "RMS dB per 0.5s:")
    print("  " + " ".join(f"{v:.0f}" for v in r))
