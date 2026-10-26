import json, sys, time, el
FLOOR = 150
MODEL = 'eleven_multilingual_v2'
VOICES = {
 'daniel': ('onwK4e9ZLuTAKqWW03F9', 'Daniel - Steady Broadcaster'),
 'river': ('SAz9YHcvj6GT2YYXdXww', 'River - Relaxed, Neutral, Informative'),
 'sarah': ('EXAVITQu4vr4xnSDxMaL', 'Sarah - Mature, Reassuring, Confident'),
 'george': ('JBFqnCBsd6RMkjVDRZzb', 'George - Warm, Captivating Storyteller'),
}
import os
LEDGER=os.environ.get('NARRATION_LEDGER','work/ledger.json')  # own count of credits spent: the API's counter lags a minute or more
START=int(os.environ.get('NARRATION_START_CREDITS','10000'))
def spent():
    return json.load(open(LEDGER))['spent'] if os.path.exists(LEDGER) else 0
def left():
    j = el.sub(); return j['character_limit'] - j['character_count']
def safe_left():
    # the API counter lags by a minute or more, so also keep our own ledger and trust the lower number
    return min(left(), START - spent())
def tts(voice, text, out, model=MODEL, rate=1.0, speed=1.0):
    import time; time.sleep(4)
    before = safe_left()
    need = int(len(text) * rate + 0.999)
    if before - need < FLOOR:
        print('REFUSED: would leave', before - need, 'credits'); return None
    vid = VOICES[voice][0]
    body = {'text': text, 'model_id': model, 'language_code': 'en',
            'voice_settings': {'stability': 0.6, 'similarity_boost': 0.75, 'style': 0.0, 'use_speaker_boost': True, 'speed': speed}}
    s, h, b = el.req('POST', f'https://api.elevenlabs.io/v1/text-to-speech/{vid}?output_format=mp3_44100_128', body, accept='audio/mpeg')
    if s != 200:
        print('TTS failed', s, b[:300]); return None
    open(out, 'wb').write(b)
    json.dump({'spent': spent() + need}, open(LEDGER,'w'))
    print(f'{out.split("/")[-1]}: {len(text)} chars, charged ~{need}, ledger left {START-spent()}, {len(b)} bytes')
    return need
