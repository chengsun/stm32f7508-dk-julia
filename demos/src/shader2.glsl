/*
zozuar ifs

https://twitter.com/zozuar/status/1641610897457135618
*/

const float PI = 3.141592653589793;

mat2 rotate2D(float r) {
    return mat2(cos(r), sin(r), -sin(r), cos(r));
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
  fragColor = vec4(0.);

  mat2 m = rotate2D(iTime * .2);
  vec3 rayDirection = normalize(vec3((gl_FragCoord.xy * 2. - iResolution.xy) / iResolution.y, 1.));
  rayDirection.xz *= m;
  rayDirection.yz *= m;

  float rayLen = 0.;

  for (int i = 0; i < 50; ++i) {
    vec3 p = rayDirection * rayLen;
    p.z += iTime / PI;

    float minDistance = 9.;
    float stepLen = 1. / 9.;

    for (int j = 0; j < 16; ++j) {
      p = mod(p, 2.) - 1.;
      p.yz *= rotate2D(PI/4.);

      float thisDistance = length(p);

      minDistance = min(minDistance, thisDistance);

      float marchDistance = thisDistance * thisDistance * .5;
      stepLen *= marchDistance;
      p /= marchDistance;
      p -= 1.;
    }

    rayLen += stepLen;

    fragColor += .01 / exp(stepLen * 1e3 + rayLen) * (2. + sin(vec4(1,4,6,9) * minDistance));
  }
}





/*****/

const float PI = 3.141592653589793;

mat2 rotate2D(float r) {
    return mat2(cos(r), sin(r), -sin(r), cos(r));
}

float fround(float x, float y) { return round(x*y)/y; }
float fround(float x, float y, float o) { return (round(x*y + o)-o)/y; }
vec2 fround(vec2 x, float y) { return round(x*y)/y; }
vec3 fround(vec3 x, float y) { return round(x*y)/y; }

bool error = false;

vec4 lookup(vec3 p) {
    float minDistance = 100.;
    float accum = 1.;

    p = mod(p, 2.);
    p = fround(p, 64.);
    for (int j = 0; j < 20; ++j) {
      p = mod(p, 2.) - 1.;
      p.yz *= rotate2D(PI/4.);

      float thisDistance = length(p);

      minDistance = min(minDistance, thisDistance);

      float marchDistance = thisDistance * thisDistance * .5;
      accum *= marchDistance;
      p /= marchDistance;
      p -= 1.;
    }

    vec3 baseColor = 1.75 + sin(vec3(3,4,6) * minDistance * 0.9);
    vec3 accumColor = .0234 * vec3(baseColor) / exp(accum * 32.);

    return vec4(fround(accumColor, 256.), fround(clamp(accum, 0., 1.), 256.));
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
  fragColor = vec4(0.);

  fragCoord = (fragCoord.xy * 2. - iResolution.xy) / iResolution.y;
  fragCoord = fround(fragCoord, 272. / 2.);
  vec2 fc = fract(fragCoord * (272. / 2. / 2.));
  float d = abs(fc.y - fc.x);

  float iTime = iTime;
  iTime = fround(iTime + 0.5, 17., d > 0.25 ? 0.5 : 0.);

  mat2 m = rotate2D(iTime * .2);
  vec3 rayDirection = vec3(fragCoord, 1.);
  rayDirection.xz *= m;
  rayDirection.yz *= m;

  float rayLen = 0.;

  for (int i = 0; i < 17; ++i) {
    vec3 p = rayDirection * rayLen;
    p.z += iTime / PI;

    vec4 lookupResult = lookup(p);

    rayLen += lookupResult.w / 8.;
    fragColor += vec4(lookupResult.xyz, 0.);
  }


  if (fragColor.x > 1. || fragColor.y > 1. || fragColor.z > 1.) error = true;

  fragColor = error ? vec4(1,0,0,1) : vec4(fragColor.xyz, 1.);
}
