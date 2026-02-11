import { motion, AnimatePresence } from "framer-motion";
import { useAppStore } from "../state/appStore";
import PlasmaOrb from "./PlasmaOrb";

const HUD = () => {
  const hudState = useAppStore((state) => state.hudState);
  const isVisible = hudState !== "idle";

  return (
    <div className="pointer-events-none absolute inset-0 flex items-end justify-center pb-6">
      <AnimatePresence>
        {isVisible && (
          <motion.div
            initial={{ opacity: 0, y: 10, scale: 0.9 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 8, scale: 0.84 }}
            transition={{ duration: 0.42, ease: [0.22, 1, 0.36, 1] }}
          >
            <PlasmaOrb state={hudState} size={106} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export default HUD;
