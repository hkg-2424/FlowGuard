# FlowGuard — Product Vision

## 1. The Core Problem: Fragmentation vs. Intentional Context Switching

Traditional productivity software treats all application and window switching as a distraction. Standard "blockers" implement blunt domain blacklists or strict timer fences that fail to reflect modern knowledge work.

Consider realistic modern workflows:

### Scenario A: Competitive Programming
A software engineer solving algorithm problems might simultaneously utilize:
- An editor or IDE (VS Code, JetBrains, Antigravity)
- A browser with tabs for Codeforces, LeetCode, or AtCoder
- Official language documentation (cppreference, docs.rs)
- ChatGPT or Gemini for algorithmic complexity queries
- A local terminal running test inputs

In traditional distraction blockers, switching between Chrome and VS Code triggers a penalty or distraction warning. In reality, all of these context switches belong to the **same intended workflow**.

### Scenario B: Technical Learning & Tutorials
A developer learning React might watch a video tutorial on YouTube while following along in a code editor and inspecting DOM trees in Chrome DevTools. 
- Switching between the IDE and the specific video tutorial is part of active learning.
- Abandoning the selected video to click on recommended clickbait, gaming clips, or social media is a genuine distraction.

FlowGuard exists to distinguish **productive context-switching within an intended workflow** from **unintended task abandonment**.

---

## 2. Product Pillars

1. **Local-First & Absolute Privacy**  
   User activity data is sensitive. FlowGuard processes everything locally. It will never upload user telemetry, browsing history, keystrokes, screenshots, or screen contents to remote clouds.

2. **Resource Intelligence Over Blind Blocking**  
   FlowGuard aims to understand not just that "Chrome is open", but which project, documentation set, or specific tutorial is part of the user's focus session.

3. **Decoupled Architecture**  
   The core intelligence of FlowGuard (workflow rules, analytics, intervention logic) is decoupled from telemetry collection mechanisms. Whether activity is provided by ActivityWatch or future native Windows API hooks, the domain logic remains identical.

4. **Zero-Overhead Engineering**  
   FlowGuard is written in high-performance Rust. It runs quietly in the background without hogging CPU cycles, memory, or battery life.
