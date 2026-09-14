"""Small-motion anatomical masks for the two supplied, seated Hunyuan meshes.

Coordinates are original glTF local coordinates (Y up, Z forward). These are
asset-specific conservative masks, not a general autorig or locomotion solution.
"""
import numpy as np

WEIGHT_SCALE = 65536


def smooth(value, low, high):
    t = np.clip((value-low)/(high-low), 0, 1)
    return t**3*(10-15*t+6*t*t)


def regions(pet, coordinates):
    x, y, z = coordinates.T
    if pet == 'naitang':
        # Only distal curled tail, not the seated haunch or the front paws.
        tail = smooth(z, -.08, .07)*(1-smooth(x, -.17, -.115))*(1-smooth(y, .085, .115))
        tip = smooth(z, .02, .15)
        left = (1-smooth(x, -.15, -.105))*smooth(y, .725, .79)
        right = smooth(x, .155, .19)*smooth(y, .75, .81)
    elif pet == 'pudding':
        tail = (1-smooth(x, -.30, -.245))*(1-smooth(z, -.20, -.12))*smooth(y, .035, .09)
        tip = smooth(y, .10, .22)
        depth = 1-smooth(z, .14, .22)
        vertical = smooth(y, .45, .51)*(1-smooth(y, .72, .80))
        left = (1-smooth(x, -.22, -.15))*depth*vertical
        right = smooth(x, .18, .25)*depth*vertical
    else:
        raise ValueError(f'Unsupported asset: {pet}')
    return {'tail': tail, 'tip': tip, 'ear.L': left, 'ear.R': right}


def partition(pet, coordinates):
    masks = regions(pet, coordinates)
    head = np.rint(smooth(coordinates[:, 1], .38, .58)*WEIGHT_SCALE).astype(np.int32)
    body = WEIGHT_SCALE-head
    left = np.rint(masks['ear.L']*head).astype(np.int32)
    right = np.rint(masks['ear.R']*head).astype(np.int32)
    tail = np.rint(masks['tail']*body).astype(np.int32)
    tip = np.rint(masks['tip']*tail).astype(np.int32)
    result = {'body_anchor': body-tail, 'head': head-left-right,
              'ear.L': left, 'ear.R': right, 'tail.base': tail-tip, 'tail.tip': tip}
    if any(np.any(w < 0) for w in result.values()):
        raise ValueError('Overlapping anatomical regions')
    assert np.all(sum(result.values()) == WEIGHT_SCALE)
    return result
