// ==============================================================================
// Nave Glow - Subtle GPU neon bloom + gentle edge vignette for Windows Terminal
// ==============================================================================

Texture2D shaderTexture : register(t0);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings : register(b0) {
    float  Time;
    float  Scale;
    float2 Resolution;
    float4 Background;
};

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET
{
    float4 base = shaderTexture.Sample(samplerState, tex);

    // Soft 8-tap neon bloom halo on bright/accent text
    float2 px = (1.6 * Scale) / max(Resolution, float2(1.0, 1.0));
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
        float lum = dot(s, float3(0.299, 0.587, 0.114));
        float bright = smoothstep(0.32, 0.90, lum);
        glow += s * bright;
    }
    glow *= 0.065;

    // Gentle edge vignette for cockpit depth
    float2 centered = tex - 0.5;
    float dist2 = dot(centered, centered);
    float vignette = 1.0 - smoothstep(0.24, 0.78, dist2) * 0.16;

    float3 finalColor = saturate((base.rgb + glow) * vignette);
    return float4(finalColor, base.a);
}
