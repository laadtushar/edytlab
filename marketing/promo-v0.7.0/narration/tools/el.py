import json, sys, urllib.request, urllib.error
import os
KEY = open(os.environ['ELEVENLABS_KEY_FILE']).read().strip()  # a file holding the key; never printed or committed
def req(method, url, body=None, accept='application/json'):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method, headers={'xi-api' + '-key': KEY, 'User-Agent': 'edytlab-dev-agent', 'Content-Type': 'application/json', 'Accept': accept})
    try:
        with urllib.request.urlopen(r, timeout=120) as resp:
            return resp.status, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read()
def sub():
    import time
    for i in range(6):
        s, h, b = req('GET', 'https://api.elevenlabs.io/v1/user/subscription')
        if s != 429: break
        time.sleep(20)
    if s != 200:
        print('sub status', s, b[:300]); sys.exit(1)
    j = json.loads(b)
    return j
if __name__ == '__main__':
    j = sub()
    print({k: j.get(k) for k in ('tier','character_count','character_limit','status','next_character_count_reset_unix','can_use_instant_voice_cloning','currency')})
    print('left', j['character_limit'] - j['character_count'])
