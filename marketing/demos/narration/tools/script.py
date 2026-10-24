# The narration lines of the three DJ demos: id, cut, the cue's span (lo, hi), when the line starts (at), the text and its source.
L = []
# ---------------- the three DJ demos (website/public/demos/dj-*.mp4). lo/hi: the cue's span, or up to where the app's own sound starts
D = 'website/lib/demos.ts caption of '
def demo(cut, id, cue, lo, hi, at, text, claim):
    L.append(dict(cut=cut, id=id, shot=cue, lo=lo, hi=hi, at=at, text=text, claim=claim, music=False))
bt = D + 'dj-beatmatched-transition'
demo('BT','bt01','BT01 title',0.0,2.467,0.40,"Blending two tracks.",bt+' (title "Beatmatch and blend two tracks")')
demo('BT','bt02','BT02 open the tracks',2.467,10.067,2.80,"First, open both tracks: Midnight Drive and Neon Rush.",bt+' (names both tracks); on-screen "Open the tracks"')
demo('BT','bt03','BT03 before (120 vs 128 BPM)',10.067,14.84,10.40,"One twenty against one twenty-eight.",bt+' (Neon Rush is about 6.7% faster; Midnight Drive is 120 BPM); on-screen "both tracks from the top, 120 against 128 BPM"')
demo('BT','bt04','BT04 what am I working with',21.933,34.867,22.50,"Ask what you are working with. Both tracks are in F major, and Neon Rush is about six point seven percent faster.",bt+' (sentence 1: "both tracks are in F major, and Neon Rush is about 6.7% faster")')
demo('BT','bt05','BT05 beatmatch',34.867,47.333,35.20,"Neon Rush is time-stretched to Midnight Drive's one twenty BPM, keeping its pitch.",bt+' (sentence 2: "time-stretched to Midnight Drive\'s 120 BPM, keeping its pitch")')
demo('BT','bt06','BT06 line up the overlap',47.333,59.6,47.65,"It starts at sixteen seconds, under the last eight bars. Claude notes its bars then run a beat behind, and offers fifteen and a half seconds instead.",bt+' (sentence 2: "started at 16 s, under Midnight Drive\'s last 8 bars; Claude points out ... a beat behind and offers ... 15.5 s")')
demo('BT','bt07','BT07 blend',59.6,82.133,60.00,"Midnight Drive fades out under a one kilohertz low-pass, while Neon Rush fades in.",bt+' (sentence 3: "fades out under a 1 kHz low-pass while Neon Rush fades in")')
demo('BT','bt08','BT08 listen (transition)',82.133,86.33,82.45,"Have a listen.",bt+' ("every playback is in real time, with sound"); on-screen "Listen: the transition"')
demo('BT','bt09','BT09 master',104.4,132.067,104.70,"For the master, Claude mixes the two to a new track, compresses it, limits at minus one dB, and raises it to about minus fourteen loofs.",bt+' (sentence 4: "mixes the two to a new track, compresses it, limits at -1 dB and raises it to about -14 LUFS")')
demo('BT','bt10','BT10 export',132.067,144.933,132.40,"Then it exports a fifty-second wave file.",bt+' (sentence 4: "exports a 50-second WAV")')
demo('BT','bt11','BT11 listen (mastered mix)',144.933,149.03,145.25,"The mastered mix.",bt+' (sentence 4); on-screen "The mastered mix, from 0:20"')
demo('BT','bt12','BT12 done',161.2,165.333,161.50,"Minus thirteen point nine loofs.",bt+' (sentence 4: "at -13.9 LUFS")')
ei = D + 'dj-extended-club-intro'
demo('EI','ei01','EI01 title',0.0,2.4,0.40,"A mixable intro.",ei+' (title "Extend an intro for mixing")')
demo('EI','ei02','EI02 open the track',2.4,9.733,2.80,"First, open the track: Solar Flare.",ei+' (names Solar Flare); on-screen "Open the tracks"')
demo('EI','ei03','EI03 before (original intro)',9.733,14.53,10.10,"First, the original intro.",'on-screen "Before: the original intro"; '+ei+' ("every playback is in real time, with sound")')
demo('EI','ei04','EI04 tempo and bar length',22.733,35.4,23.05,"Ask for its tempo and bar length without changing anything: 124 BPM, a 1.935-second bar, and the first downbeat at 0.476 seconds.",ei+' (sentence 1: "124 BPM, a 1.935-second bar, the first downbeat at 0.476 s")')
demo('EI','ei05','EI05 extend the intro',35.4,51.6,38.50,"The drums-only first eight bars are copied from that downbeat and pasted after themselves: a sixteen-bar intro, and the track grows from 32 to 47.5 seconds.",ei+' (sentence 2: "the drums-only first 8 bars are copied ... pasted after themselves, so the intro runs 16 bars and the track grows from 32 to 47.5 seconds")')
demo('EI','ei06','EI06 filter into the drop',51.6,66.333,52.20,"A 200 hertz high-pass holds the low end back until the drop at 31.45 seconds. Claude suggests 120 to 150 hertz if the intro sounds too thin.",ei+' (sentence 3: "A 200 Hz high-pass holds the low end back until the drop at 31.45 s, and Claude suggests 120-150 Hz if the intro sounds too thin")')
demo('EI','ei07','EI07 fade in and export',66.333,80.467,66.90,"The first four bars fade in, and the track is exported as a wave file. Claude flags that it peaks right at full scale.",ei+' (sentence 4: "The first 4 bars fade in and the track is exported as a WAV; Claude flags that it peaks at -0.0003 dBFS, right at full scale")')
demo('EI','ei08','EI08 listen (intro fades in)',80.467,85.13,80.90,"Listen to the new intro.",ei+' (sentence 5: "You hear the new intro"); on-screen "Listen: the new intro fades in"')
demo('EI','ei09','EI09 listen (the drop, 0:31)',94.2,98.48,94.60,"Wait for the drop.",ei+' (sentence 5: "then the drop at 0:31"); on-screen "Listen: the low end arrives at the drop (0:31)"')
demo('EI','ei10','EI10 done',110.733,114.867,111.10,"A sixteen-bar intro, exported.",ei+' (sentence 2 and 4); on-screen "a 16-bar intro"')
mm = D + 'dj-mini-mix'
demo('MM','mm01','MM01 title',0.0,2.467,0.40,"A three-track mini-mix.",mm+' (title "A three-track mini-mix")')
demo('MM','mm02','MM02 open the tracks',2.467,10.267,2.80,"First, open the three tracks.",mm+' (three tracks: Midnight Drive, Neon Rush, Solar Flare); on-screen "Open the tracks"')
demo('MM','mm03','MM03 match to 124 BPM',10.267,24.6,11.00,"Midnight Drive and Neon Rush are time-stretched to 124 BPM, keeping their pitch. Claude checks that their beats now line up with Solar Flare's.",mm+' (sentence 1)')
demo('MM','mm04','MM04 sequence with 4-bar overlaps',24.6,46.667,25.20,"Each track comes in about four bars before the one before it ends, with a straight-line crossfade over each overlap. Claude brings Neon Rush in on Solar Flare's bar line, which makes that overlap 8.77 seconds.",mm+' (sentence 2: "about 4 bars before the one before it ends ... straight-line crossfade ... Neon Rush on Solar Flare\'s bar line ... overlap 8.77 s")')
demo('MM','mm05','MM05 loudness and limiter',46.667,65.467,47.20,"Each track is brought to about minus 14 loofs, with a limiter at minus one dB on the master. Claude notes that the fades were counted in the measurement.",mm+' (sentence 3: "about -14 LUFS, with a limiter at -1 dB on the master, and Claude notes that the fades were counted in the measurement")')
demo('MM','mm06','MM06 listen (first blend)',65.467,70.25,65.90,"The first blend.",mm+' (sentence 4: "You hear both transitions"); on-screen "Listen: Midnight Drive into Solar Flare (0:23)"')
demo('MM','mm07','MM07 listen (second blend)',83.4,87.68,83.80,"And the second.",mm+' (sentence 4); on-screen "Listen: Solar Flare into Neon Rush (0:46)"')
demo('MM','mm08','MM08 export',100.8,114.0,101.20,"Then the mix is exported as a 79-second wave file.",mm+' (sentence 4: "exported as a 79-second WAV")')
