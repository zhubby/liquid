import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  allowedDevOrigins: ["*.*.*.*", "localhost", "*.localhost", "::1"],
  output: "standalone",
};

export default nextConfig;
