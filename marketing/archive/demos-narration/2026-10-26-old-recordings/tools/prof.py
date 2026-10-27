import subprocess, sys, numpy as np
def load(path, sr=48000):
    raw = subprocess.run(['ffmpeg','-v','error','-i',path,'-vn','-ac','2','-ar',str(sr),'-f','f32le','-'],capture_output=True).stdout
    return np.frombuffer(raw, dtype=np.float32).reshape(-1,2)
if __name__=='__main__':
    x = load(sys.argv[1]); win=float(sys.argv[2]); n=int(48000*win)
    out=[]
    for i in range(0, len(x), n):
        c = x[i:i+n]; r = np.sqrt((c**2).mean()+1e-12)
        out.append('%d:%d' % (i/48000, round(20*np.log10(r))))
    print(' '.join(out))
