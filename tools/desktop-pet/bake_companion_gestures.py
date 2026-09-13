"""Bake restrained eye/chest/head motion from the accepted character artwork.

Only local anatomy changes. No whole-pet zoom, crossfade, silhouette breathing,
or anthropomorphic waving. All resulting actions remain ordinary APNG files.
"""
import argparse
import json
import math
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter
from export_apng import write_apng

EYES = {
    "naitang": [(92,69,14,16,11,12,-2,8),(123,58,12,15,10,12,6,9)],
    "pudding": [(91,52,12,14,8,9,0,6),(123,44,11,13,8,9,1,8)],
}

def blink(neutral, closed, amount, pet):
    if amount <= 0: return neutral.copy()
    out = neutral.copy(); original = neutral.load(); source = closed.load(); pixels = out.load()
    for x0,y0,rx,ry,ax,ay,dx,dy in EYES[pet]:
        for y in range(y0-ry,y0+ry+1):
            for x in range(x0-rx,x0+rx+1):
                distance=math.hypot((x-x0)/rx,(y-y0)/ry)
                if distance>=1: continue
                aperture_y=ay*(1-amount)
                aperture=math.hypot((x-x0)/ax,(y-y0)/aperture_y) if aperture_y>.1 else math.inf
                weight=min(1,(1-distance)/.12)*max(0,min(1,(aperture-.9)/.15))*min(1,amount*4)
                dst=original[x,y]; src=source[x+dx,y+dy]
                if src[3]: pixels[x,y]=tuple(round(dst[c]*(1-weight)+src[c]*weight) for c in range(3))+(dst[3],)
    return out

def breath(frame, amount, pet):
    box=(82,116,146,172) if pet=="naitang" else (75,98,140,168)
    crop=frame.crop(box); center=crop.width/2; factor=1+amount
    warped=crop.transform(crop.size,Image.Transform.AFFINE,(1/factor,0,center-center/factor,0,1,0),Image.Resampling.BICUBIC)
    mask=Image.new("L",crop.size); ImageDraw.Draw(mask).ellipse((5,4,crop.width-5,crop.height-4),fill=255);mask=mask.filter(ImageFilter.GaussianBlur(5))
    merged=Image.composite(warped,crop,mask);merged.putalpha(crop.getchannel("A"))
    out=frame.copy();out.paste(merged,box);return out

def head_pose(frame, angle, lift, pet):
    if abs(angle)<.001 and abs(lift)<.001:return frame.copy()
    cut=116 if pet=="naitang" else 88
    head=frame.copy();head.putalpha(Image.new("L",frame.size,0))
    head.paste(frame.crop((0,0,192,cut+12)),(0,0))
    head=head.rotate(angle,Image.Resampling.BICUBIC,center=(112,cut),translate=(0,round(lift)))
    body=frame.copy();body.paste((0,0,0,0),(0,0,192,cut))
    body.alpha_composite(head);return body

def pad(frame):
    out=Image.new("RGBA",(256,208));out.paste(frame,(32,0));return out

def bake(source, output):
    pet=source.name;atlas=Image.open(source/"spritesheet.webp").convert("RGBA")
    neutral=atlas.crop((0,0,192,208));closed=atlas.crop((192,0,384,208))
    clips={}
    idle=[neutral]; times=[180]
    for cycle in range(2):
        for i in range(1,49):
            amount=.022*math.sin(math.pi*i/48)**2
            idle.append(breath(neutral,amount,pet));times.append(42)
    for amount,duration in [(.5,35),(1,80),(.9,35),(.5,35),(.08,40),(0,200)]:
        idle.append(blink(neutral,closed,amount,pet));times.append(duration)
    clips["idle"]=write_apng(output/"idle.apng",[pad(frame) for frame in idle],times)
    for name,angle,lift,hold in [("waving",-2,-2,120),("jumping",-3,-3,180),("failed",2,2,320),("review",3,0,220)]:
        frames=[neutral]
        for i in range(1,25):
            wave=math.sin(math.pi*i/24)**2
            frames.append(head_pose(neutral,angle*wave,lift*wave,pet))
        frames.append(neutral)
        durations=[42]*len(frames);durations[0]=100;durations[12]=hold;durations[-1]=180
        clips[name]=write_apng(output/f"{name}.apng",[pad(frame) for frame in frames],durations)
    for name in ("running","waiting"):
        clips[name]=write_apng(output/f"{name}.apng",[pad(frame) for frame in idle],times)
    return clips

if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("source",type=Path);parser.add_argument("output",type=Path)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    clips=bake(args.source,args.output)
    (args.output/"gestures.json").write_text(json.dumps(clips,indent=2)+"\n")
    print(json.dumps({key:len(value["durationsMs"]) for key,value in clips.items()}))
