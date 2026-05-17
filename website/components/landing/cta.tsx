"use client";

import Link from "next/link";
import { Apple, Download } from "lucide-react";
import { motion, useMotionValue, useSpring, useTransform } from "framer-motion";
import { useRef } from "react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";
import { FadeIn } from "./fade-in";

function MagneticButton({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const rawX = useMotionValue(0);
  const rawY = useMotionValue(0);
  const x = useSpring(rawX, { stiffness: 200, damping: 20 });
  const y = useSpring(rawY, { stiffness: 200, damping: 20 });

  function onMove(e: React.MouseEvent<HTMLDivElement>) {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect) return;
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    rawX.set((e.clientX - cx) * 0.3);
    rawY.set((e.clientY - cy) * 0.3);
  }

  function onLeave() {
    rawX.set(0);
    rawY.set(0);
  }

  return (
    <motion.div
      ref={ref}
      style={{ x, y }}
      onMouseMove={onMove}
      onMouseLeave={onLeave}
      whileTap={{ scale: 0.96 }}
      className={className}
    >
      {children}
    </motion.div>
  );
}

export function CTA() {
  return (
    <section className="relative overflow-hidden border-t border-border/50 bg-secondary/20 py-24 md:py-32">
      {/* Pulsing background orb */}
      <motion.div
        className="pointer-events-none absolute inset-0 flex items-center justify-center"
        initial={{ opacity: 0 }}
        whileInView={{ opacity: 1 }}
        viewport={{ once: true }}
        transition={{ duration: 1 }}
      >
        <motion.div
          className="size-96 rounded-full bg-primary/8 blur-3xl"
          animate={{ scale: [1, 1.15, 1], opacity: [0.5, 0.8, 0.5] }}
          transition={{ duration: 4, repeat: Infinity, ease: "easeInOut" }}
        />
      </motion.div>

      <div className="container relative">
        <FadeIn className="mx-auto max-w-2xl text-center">
          <motion.h2
            className="text-balance text-3xl font-bold tracking-tight sm:text-4xl md:text-5xl"
            initial={{ opacity: 0, y: 24 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ duration: 0.65, ease: [0.21, 0.47, 0.32, 0.98] }}
          >
            Ready to edit differently?
          </motion.h2>
          <motion.p
            className="mx-auto mt-4 max-w-xl text-pretty text-lg text-muted-foreground"
            initial={{ opacity: 0, y: 16 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.12, duration: 0.6, ease: [0.21, 0.47, 0.32, 0.98] }}
          >
            Free in BYO-key mode. Your audio stays on your machine. Download,
            plug in an API key, and describe your first edit.
          </motion.p>
          <motion.div
            className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row"
            initial={{ opacity: 0, y: 16 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.24, duration: 0.6, ease: [0.21, 0.47, 0.32, 0.98] }}
          >
            <MagneticButton className="w-full sm:w-auto">
              <Button asChild size="lg" className="glow w-full">
                <Link href={siteConfig.releases}>
                  <Apple className="size-4" />
                  Download for Mac
                </Link>
              </Button>
            </MagneticButton>
            <MagneticButton className="w-full sm:w-auto">
              <Button
                asChild
                size="lg"
                variant="outline"
                className="w-full"
              >
                <Link href={siteConfig.releases}>
                  <Download className="size-4" />
                  Download for Windows
                </Link>
              </Button>
            </MagneticButton>
          </motion.div>
          <motion.p
            className="mt-4 text-xs text-muted-foreground"
            initial={{ opacity: 0 }}
            whileInView={{ opacity: 1 }}
            viewport={{ once: true }}
            transition={{ delay: 0.4, duration: 0.5 }}
          >
            Signed installers · Universal Mac (Apple Silicon + Intel) · Windows 10/11
          </motion.p>
        </FadeIn>
      </div>
    </section>
  );
}
