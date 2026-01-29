// Port of Preetham Atmospheric Sky Shader to Bevy WGSL
// Based on the Preetham Model

struct SkyMaterial {
    sun_position: vec3<f32>,
    turbidity: f32,
    rayleigh: f32,
    mie_coefficient: f32,
    mie_directional_g: f32,
}

@group(2) @binding(0)
var<uniform> material: SkyMaterial;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) camera_pos: vec3<f32>,
}

const PI: f32 = 3.141592653589793;
const UP: vec3<f32> = vec3<f32>(0.0, 1.0, 0.0);

// Precomputed Rayleigh scattering coefficients
const primaries: vec3<f32> = vec3<f32>(6.8e-7, 5.5e-7, 4.5e-7); // RGB Wavelengths in meters
const refractiveIndex: f32 = 1.0003;
const numMolecules: f32 = 2.545e25;
const depolarizationFactor: f32 = 0.035;

fn totalRayleigh(lambda: vec3<f32>) -> vec3<f32> {
    let n2_minus_1 = refractiveIndex * refractiveIndex - 1.0;
    let denom = 3.0 * numMolecules * pow(lambda, vec3<f32>(4.0)) * (6.0 - 7.0 * depolarizationFactor);
    return (8.0 * pow(PI, 3.0) * pow(n2_minus_1, 2.0) * (6.0 + 3.0 * depolarizationFactor)) / denom;
}

fn totalMie(lambda: vec3<f32>, T: f32) -> vec3<f32> {
    let c = 0.2 * T * 1e-17;
    let mieV: f32 = 4.0; // Standard value for mieV
    let mieK: vec3<f32> = vec3<f32>(0.686, 0.678, 0.666); // Standard K
    return 0.434 * c * PI * pow((2.0 * PI) / lambda, vec3<f32>(mieV - 2.0)) * mieK;
}

fn rayleighPhase(cosTheta: f32) -> f32 {
    return (3.0 / (16.0 * PI)) * (1.0 + cosTheta * cosTheta);
}

fn henyeyGreensteinPhase(cosTheta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 / (4.0 * PI)) * ((1.0 - g2) / pow(1.0 - 2.0 * g * cosTheta + g2, 1.5));
}

fn sunIntensity(zenithAngleCos: f32) -> f32 {
    let sunIntensityFactor: f32 = 1000.0;
    let sunIntensityFalloffSteepness: f32 = 0.98;
    let cutoffAngle = PI / 1.95;
    return sunIntensityFactor * max(0.0, 1.0 - exp(-((cutoffAngle - acos(zenithAngleCos)) / sunIntensityFalloffSteepness)));
}

fn Uncharted2Tonemap(W: vec3<f32>) -> vec3<f32> {
    let A: f32 = 0.15;
    let B: f32 = 0.50;
    let C: f32 = 0.10;
    let D: f32 = 0.20;
    let E: f32 = 0.02;
    let F: f32 = 0.30;
    return ((W * (A * W + C * B) + D * E) / (W * (A * W + B) + D * F)) - E / F;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let viewDirection = normalize(input.world_position - input.camera_pos);
    let sunDirection = normalize(material.sun_position);
    
    // Rayleigh
    let sunfade = 1.0 - clamp(1.0 - exp(material.sun_position.y / 450000.0), 0.0, 1.0);
    let rayleighCoefficient = material.rayleigh - (1.0 * (1.0 - sunfade));
    let betaR = totalRayleigh(primaries) * rayleighCoefficient;
    
    // Mie
    let betaM = totalMie(primaries, material.turbidity) * material.mie_coefficient;
    
    // Optical length
    let zenithAngle = acos(max(0.0, dot(UP, viewDirection)));
    let denom = cos(zenithAngle) + 0.15 * pow(max(0.001, 93.885 - ((zenithAngle * 180.0) / PI)), -1.253);
    
    let rayleighZenithLength: f32 = 8400.0;
    let mieZenithLength: f32 = 1250.0;
    let sR = rayleighZenithLength / denom;
    let sM = mieZenithLength / denom;
    
    // Extinction factor
    let Fex = exp(-(betaR * sR + betaM * sM));
    
    // In-scattering
    let cosTheta = dot(viewDirection, sunDirection);
    let betaRTheta = betaR * rayleighPhase(cosTheta * 0.5 + 0.5);
    let betaMTheta = betaM * henyeyGreensteinPhase(cosTheta, material.mie_directional_g);
    
    let sunE = sunIntensity(dot(sunDirection, UP));
    var Lin = pow(sunE * ((betaRTheta + betaMTheta) / (betaR + betaM)) * (1.0 - Fex), vec3<f32>(1.5));
    Lin *= mix(vec3<f32>(1.0), pow(sunE * ((betaRTheta + betaMTheta) / (betaR + betaM)) * Fex, vec3<f32>(0.5)), clamp(pow(1.0 - dot(UP, sunDirection), 5.0), 0.0, 1.0));
    
    // Solar disc
    let sunAngularDiameterDegrees: f32 = 0.008; // Small sun
    let sunAngularDiameterCos = cos(sunAngularDiameterDegrees);
    let sundisk = smoothstep(sunAngularDiameterCos, sunAngularDiameterCos + 0.00002, cosTheta);
    
    var L0 = vec3<f32>(0.1) * Fex;
    L0 += sunE * 19000.0 * Fex * sundisk;
    
    var texColor = Lin + L0;
    texColor *= 0.04;
    texColor += vec3<f32>(0.0, 0.001, 0.0025) * 0.3;
    
    // Tonemapping
    let luminance: f32 = 1.0;
    let tonemapWeighting: f32 = 9.29; // Matches Three.js default
    let whiteScale = 1.0 / Uncharted2Tonemap(vec3<f32>(tonemapWeighting));
    let curr = Uncharted2Tonemap((log2(2.0 / pow(luminance, 4.0))) * texColor);
    
    let color = curr * whiteScale;
    let retColor = pow(color, vec3<f32>(1.0 / (1.2 + (1.2 * sunfade))));

    return vec4<f32>(retColor, 1.0);
}
