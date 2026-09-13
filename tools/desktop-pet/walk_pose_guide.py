"""Physically registered four-beat walk guide for image-model limb rendering.

Colored bones are INPUT GUIDANCE ONLY, never a shipped pet asset.
"""
import argparse
import json
import math
from pathlib import Path
from PIL import Image, ImageDraw

STRIDE = 64
GROUND = 202
LEGS = [
    ("far-hind",94,134,.5,-1,(65,130,255)),
    ("far-front",188,126,.75,1,(80,220,220)),
    ("near-hind",82,133,0,-1,(250,170,70)),
    ("near-front",176,125,.25,1,(245,95,135)),
]

def foot(phase, offset, root_x):
    p=(phase+offset)%1
    if p<.25:
        t=p/.25; smooth=t*t*(3-2*t)
        return root_x-24+48*smooth, GROUND-13*math.sin(math.pi*t), False
    return root_x+24-STRIDE*(p-.25), GROUND, True

def joint(a,b,length,bend):
    dx,dy=b[0]-a[0],b[1]-a[1]; distance=math.hypot(dx,dy)
    height=math.sqrt(max(0,length*length-distance*distance/4))
    return (a[0]+dx/2-bend*dy/max(distance,.001)*height,
            a[1]+dy/2+bend*dx/max(distance,.001)*height)

def make(body_path, output):
    body=Image.open(body_path).convert("RGBA")
    if body.size != (256,208):raise ValueError("Expected registered 256x208 body")
    sheet=Image.new("RGBA",(1024,1536),(255,0,255,255))
    mask=Image.new("RGBA",sheet.size,(0,0,0,255)); edit=ImageDraw.Draw(mask)
    poses=[]
    for i in range(24):
        phase=i/24; bob=round(.8*math.cos(4*math.pi*phase))
        frame=Image.new("RGBA",(256,208))
        top=body.crop((0,0,256,132));frame.alpha_composite(top,(0,bob))
        draw=ImageDraw.Draw(frame); legs=[]
        for name,x,y,offset,bend,color in LEGS:
            fx,fy,planted=foot(phase,offset,x)
            hip=(x,y+bob); ankle=(fx,fy-9)
            knee=joint(hip,ankle,40 if "front" in name else 36,bend)
            draw.line([hip,knee,ankle], fill=color, width=12 if name.startswith("far") else 16)
            for cx,cy in (hip,knee,ankle):draw.ellipse((cx-7,cy-7,cx+7,cy+7),fill=color)
            draw.ellipse((fx-10,fy-10,fx+11,fy),fill=color)
            legs.append({"name":name,"hip":hip,"knee":knee,"foot":[fx,fy],"planted":planted})
        at=(i%4*256,i//4*256+24)
        sheet.alpha_composite(frame,at)
        edit.rectangle((at[0],at[1]+112,at[0]+255,at[1]+207),fill=(0,0,0,0))
        poses.append({"phase":phase,"rootX":STRIDE*phase,"bodyY":bob,"legs":legs})
    output.mkdir(parents=True,exist_ok=True)
    sheet.save(output/"guide.png");mask.save(output/"edit-mask.png")
    (output/"poses.json").write_text(json.dumps({"stridePx":STRIDE,"frameDurationMs":42,"poses":poses},indent=2)+"\n")

if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("body",type=Path);parser.add_argument("output",type=Path)
    args=parser.parse_args();make(args.body,args.output)
