// ==============================================================================
// Nave CRT - Retro-futuristic micro scanlines + neon bloom + vignette
// ==============================================================================

Texture2D shaderTexture : register(t0);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings {
    float  Time;
    float  Scale;
    float2 Resolution;
    float4 Background;
};

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET
{
    float2 px = Scale / max(Resolution, float2(1.0, 1.0));

    // Subtle RGB subpixel chromatic shift
    float r = shaderTexture.Sample(samplerState, tex + float2(px.x * 0.45, 0.0)).r;
    float g = shaderTexture.Sample(samplerState, tex).g;
    float b = shaderTexture.Sample(samplerState, tex - float2(px.x * 0.45, 0.0)).b;
    float a = shaderTexture.Sample(samplerState, tex).a;
    float3 base = float3(r, g, b);

    // Soft phosphor bloom
    float3 glow =
        shaderTexture.Sample(samplerState, tex + float2( px.x * 1.6, 0)).rgb +
        shaderTexture.Sample(samplerState, tex + float2(-px.x * 1.6, 0)).rgb +
        shaderTexture.Sample(samplerState, tex + float2(0,  px.y * 1.6)).rgb +
        shaderTexture.Sample(samplerState, tex + float2(0, -px.y * 1.6)).rgb;
    glow *= 0.045;

    // Ultra-fine horizontal scanlines
    float scan = 0.955 + 0.045 * sin(tex.y * Resolution.y * 3.14159);

    // Edge vignette
    float2 centered = tex - 0.5;
    float vignette = 1.0 - dot(centered, centered) * 0.32;

    float3 color = saturate((base + glow) * scan * vignette);
    return float4(color, a);
}
