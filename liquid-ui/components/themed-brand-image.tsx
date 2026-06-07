"use client";

import Image, { type ImageProps } from "next/image";
import { useTheme } from "next-themes";

type ThemedBrandImageProps = Omit<ImageProps, "src"> & {
  src: string;
  darkSrc: string;
};

export function ThemedBrandImage({
  src,
  darkSrc,
  alt,
  ...props
}: ThemedBrandImageProps) {
  const { resolvedTheme } = useTheme();
  const activeSrc = resolvedTheme === "dark" ? darkSrc : src;

  return <Image {...props} src={activeSrc} alt={alt} />;
}
