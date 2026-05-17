"use client";

import { useEffect, useRef, useState } from "react";
import { useInView, animate } from "framer-motion";

interface AnimatedCounterProps {
  value: string;
  className?: string;
}

export function AnimatedCounter({ value, className }: AnimatedCounterProps) {
  const ref = useRef<HTMLSpanElement>(null);
  const isInView = useInView(ref, { once: true });
  const [display, setDisplay] = useState<string>("0");

  const numericMatch = value.match(/^(\d+)(%?)$/);
  const isNumeric = !!numericMatch;
  const target = isNumeric ? parseInt(numericMatch![1], 10) : null;
  const suffix = isNumeric ? numericMatch![2] : "";

  useEffect(() => {
    if (!isInView || target === null) {
      setDisplay(value);
      return;
    }
    const controls = animate(0, target, {
      duration: 1.8,
      ease: [0.16, 1, 0.3, 1],
      onUpdate: (v) => setDisplay(Math.round(v) + suffix),
    });
    return controls.stop;
  }, [isInView, target, suffix, value]);

  return (
    <span ref={ref} className={className}>
      {isNumeric ? display : value}
    </span>
  );
}
