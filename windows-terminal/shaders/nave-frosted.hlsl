// ==============================================================================
// Nave Frosted - Dark-Backed Frosted Glass (Acrylic over dark screen look)
// 100% opaque to windows behind it (immune to white browser/PDF screens!),
// while preserving velvety dark acrylic tint, soft ambient depth, matte grain,
// and subtle neon text bloom for Windows Terminal.
// ==============================================================================

Texture2D shaderTexture : register(t0);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings : register(b0) {
    float  Time;
    float  Scale;
    float2 Resolution;
    float4 Background;
};

// Artifact-free static 2D hash (matches Windows Acrylic matte micro-grain)
float acrylicGrain(float2 pixelCoord)
{
    float3 p3 = frac(float3(pixelCoord.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z) - 0.5;
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET
{
    float4 base = shaderTexture.Sample(samplerState, tex);
    float2 res = max(Resolution, float2(1.0, 1.0));
    float2 px = (1.6 * Scale) / res;

    // 1. Soft 8-tap neon bloom halo on bright/accent text (same signature as nave-glow)
    float3 glow = float3(0.0, 0.0, 0.0);
    float2 offsets[8] = {
        float2( 1.0,  0.0), float2(-1.0,  0.0),
        float2( 0.0,  1.0), float2( 0.0, -1.0),
        float2( 0.7,  0.7), float2(-0.7,  0.7),
        float2( 0.7, -0.7), float2(-0.7, -0.7)
    };
    [unroll]
    for (int i = 0; i < 8; i++)
    {
        float3 s = shaderTexture.Sample(samplerState, tex + offsets[i] * px).rgb;
        float sl = dot(s, float3(0.299, 0.587, 0.114));
        glow += s * smoothstep(0.32, 0.90, sl);
    }
    glow *= 0.062;

    // 2. Mask targeting background & deep shadow regions (keeps text/syntax 100% sharp)
    float3 diff = abs(base.rgb - Background.rgb);
    float distBg = max(diff.r, max(diff.g, diff.b));
    float pureBg = 1.0 - smoothstep(0.010, 0.080, distBg);
    float baseLum = dot(base.rgb, float3(0.299, 0.587, 0.114));
    float darkZone = 1.0 - smoothstep(0.045, 0.19, baseLum);
    float glassMask = max(pureBg, darkZone * 0.36);

    // 3. Deep smoked-obsidian "dark screen behind 92% Acrylic" atmospheric backdrop
    //    Stays close to obsidian (#0C0F1A-#121625) while preserving frosted glass depth
    float3 bgSeed = max(Background.rgb, float3(0.025, 0.028, 0.046));
    float3 baseTint = lerp(float3(0.013, 0.015, 0.024), bgSeed * 0.42 + float3(0.008, 0.010, 0.016), 0.55);

    // Subtle blurred dark ambient pools (mimics dark IDE/desktop softly blurred behind glass)
    float2 aspect = float2(res.x / res.y, 1.0);
    float2 d1 = (tex - float2(0.20, 0.24)) * aspect;
    float2 d2 = (tex - float2(0.78, 0.70)) * aspect;
    float2 d3 = (tex - float2(0.55, 0.12)) * aspect;
    float pool1 = exp(-dot(d1, d1) * 2.1);
    float pool2 = exp(-dot(d2, d2) * 2.4);
    float pool3 = exp(-dot(d3, d3) * 3.0);

    float3 ambient =
        pool1 * float3(0.009, 0.012, 0.020) +
        pool2 * float3(0.010, 0.007, 0.017) +
        pool3 * float3(0.005, 0.009, 0.014);

    // Whisper-soft top-left glass specular sheen
    float sheen = smoothstep(1.15, 0.0, tex.x * 0.55 + tex.y * 0.95) * 0.007;

    // Fine static matte acrylic micro-grain (silky frosted texture, zero chalkiness)
    float grain = acrylicGrain(floor(pos.xy)) * 0.016;

    float3 frostedLift = baseTint + ambient + float3(sheen * 0.85, sheen * 0.95, sheen * 1.25) + grain;
    float3 color = base.rgb + frostedLift * glassMask + glow;

    // 4. Gentle cockpit vignette
    float2 centered = tex - 0.5;
    float vignette = 1.0 - smoothstep(0.22, 0.78, dot(centered, centered)) * 0.18;

    return float4(saturate(color * vignette), base.a);
}
