# Live sharing: share a board and join a session

*Version française : [fr/SHARING.md](fr/SHARING.md)*

Inkboard can share a board **live**: you click **Share**, you get a link, and the people who open it (after your approval) see and edit the same board at the same time, with everybody's cursor. You are the "server": the board stays on your computer and the session ends when you close it.

> **Status.** Live sharing is a feature under development (pre-release builds). This page describes what the code does today; what is not done yet is written as such. Self-hosting the connection service is covered in [RELAY.md](RELAY.md).

## At a glance

- **Nothing connects until you click "Start sharing" (host) or "Join" (guest).** Inkboard stays 100 % offline otherwise.
- The link is a **secret**: share it only with the people you want to invite.
- Every newcomer needs **your approval**. You can remove anybody at any time.
- Everything is **end-to-end encrypted**; the connection service (the "relay") only carries encrypted data.
- If you are on a restricted network (company, school), see [Restricted networks](#restricted-networks-company-school).

---

## Share a board (host)

1. Open the board you want to share.
2. Click **Share** in the title bar. The Share window opens and says "Not shared".
3. Optional settings, before or during the session:
   - **Your name**: the name shown on your cursor and in the participant list (default "Host"; Inkboard never uses your Windows account name). It is remembered.
   - **Participants (maximum)**: the maximum number of people in the session, **you included**. Default 10; you can set 2 to 64. Inkboard is designed and tested for about 20 people.
   - **Let people with the read-only link in without asking**: off by default. When on, anybody who opens the read-only link enters without an approval window (they can only look).
4. Click **Start sharing**. Inkboard now contacts the connection service, which can take a few seconds.
5. Two links appear, each with a **Copy** button: the **Edit link** and the **Read-only link**. Paste the one you want in a message, an e-mail, a chat.

From then on, a green dot and a number ("● 3") in the title bar show that the session is active and how many people are in it (you included). Hover it for details.

### Edit link or read-only link?

| | Edit link | Read-only link |
|---|---|---|
| What the person can do | Edit the board (if you accept them as editor) | Look, move around, point with the laser; cannot change anything |
| Who should get it | People you trust to edit | Audience, reviewers |

- **Never post the edit link publicly** (a public chat room, a social network, a web page). Anybody who has it can ask to enter with edit rights. You still have to approve each person, but a flood of requests is unpleasant, and a link that circulates is a link you no longer control.
- The link is a secret: it contains the key that encrypts the session and connection hints for your computer. Treat it like a password.
- The access actually granted is the lower of what the link allows and what you choose: someone who arrives with the read-only link cannot be accepted as editor ("Accept as editor" is greyed out).
- Rights are enforced by your computer, not only hidden in the interface: a read-only guest's changes are refused.

### Approve, refuse, remove

When somebody opens a link, a window **Join requests** shows "*Anna* wants to join" (the Share window also opens, and a notification appears) with three buttons:

- **Accept as editor** (only for people who came with the edit link),
- **Accept as viewer**,
- **Decline**.

The name is what the guest typed. It is only a label: it is not proof of who they are. If two people choose the same name, Inkboard shows "Anna" and "Anna (2)".

In the Share window, the **Participants** list shows each person with a coloured dot. Next to each guest:

- **Rename**: change the displayed name (useful if somebody picked a confusing name).
- The access button (**Can edit** / **Can view**): click to switch the person to the other right, at any time. The guest is told ("The host changed your access to read-only", "The host lets you edit now").
- **Follow**: your view follows that person (same page, same visible area). Click **Stop following** to get your view back.
- **Remove**: removes the person from the session. They cannot come back with the same link on the same installation.

A guest who loses the connection shows "(absent)" and keeps their place for 60 seconds.

### If a link leaks: new links

Click **New links (the old ones stop working)**. Both links are replaced; the old ones stop working. People already in the session stay; remove the ones you do not want.

### Stop sharing

Click **Stop sharing** in the Share window (or quit Inkboard). Everybody is told that the host ended the session, and all connections are closed. Your board is yours, as before; nothing was written to your files by the guests. Inkboard does not save the board for you: save it as usual.

---

## Join a session (guest)

1. Get a link from the host. It looks like `inkboard://join#…` (or `inkboard://join/…`).
2. Either:
   - **click the link**: if Inkboard is installed, it opens the **Join a session** window with the link filled in ("Link received"); or
   - in Inkboard, open the main menu and choose **Join a session…**, then paste the link in **Invitation link**.
   If clicking the link does nothing (for example the installer did not register the `inkboard://` links on your system, which has not been verified on every platform), use the menu entry.
3. Type **Your name** (it is pre-filled with the last name you used).
4. Click **Join**. The window shows "Session of *host name*" and "Waiting for the host's approval…". Click **Cancel** to give up.
5. When the host accepts, the host's board replaces yours on screen, and a blue bar at the top right shows "Session of *host name*" (and "· Read-only" if that is your access) with a **Leave the session** button.

Your own board is **set aside, not lost**: when you leave the session, it comes back exactly as it was. While you are in a session, the "New board" and "Open" entries are unavailable, and nothing is auto-saved from the host's board to your disk.

### Working together

- You see the other people's **cursors** with their names, and the objects they have selected in their colour.
- **Edits are live.** If two people change the *same object* at the same moment, the change that reaches the host last wins for that object. There is no character-by-character co-editing of the same text.
- **Undo / redo in a group: you only undo your own actions.** Ctrl+Z undoes *your* last change, never somebody else's, and the host's Ctrl+Z only undoes the host's own changes. If somebody else has modified an object since your change, that object is left as it is (it is not undone), and you may get a notice. Redo works the same way.
- The **laser** works for everybody, including read-only guests.
- If the connection drops, an amber bar says **Reconnecting…**. Your changes are kept and sent when the connection is back; the board stays on screen as it was. A short cut (several seconds to a minute) is absorbed; after about 2 minutes without any answer, the session ends with "The connection was lost for too long."
- If the host closes the session, a red bar says so, with **Save a copy of the board** (choose where to save a copy of the shared board as a normal `.inkboard` file) and **Leave**. You can also use **Save** in the main menu at any time to save a copy; it never overwrites your own file.

### Leave

Use **Leave the session** (blue bar, or main menu). Your own board comes back.

---

## Invite someone who has nothing installed: join from a browser

A guest can join **from a web page**, with nothing to install (Chrome or Edge on a computer or a phone; see [WEB.md](WEB.md) for what was tested).

**The link.** It is the address of the Inkboard web page followed by `#` and the invitation ticket: `https://<address of the page>/#<ticket>`. The ticket is the part of the host's link after `inkboard://join#` (or `inkboard://join/`). It stays in the part of the address after `#`, which a browser never sends to a web server; the page also removes it from the address bar as soon as it has read it, so it does not stay in the browser history. In the Share window, **Copy web link** builds it for you (page address `https://victorletirand.github.io/inkboard/`; set the environment variable `INKBOARD_WEB_URL` to use another address, for example for tests on a local network). A guest can also be given the page address and the native link, which they paste in the **Join a session…** window of the page (the page accepts `inkboard://join#…`, `https://…#…` and the bare ticket).

**For the guest:**

1. Open the link. The page loads (about 5 MB the first time, then your browser keeps it) and a **Join a session** window opens with the link already in it.
2. Type **Your name** (the name of your last visit is pre-filled; it is kept in this browser only) and click **Join** (or press Enter).
3. Wait for the host's approval, exactly as in the application. Then you see and edit the board, with everybody's cursor and the agent's.
4. To keep a copy: **Save** (Ctrl+S) downloads a `.inkboard` file; the export entries of the main menu download PNG, SVG and others. If the host closes the session, the red bar offers **Save a copy of the board**.

**Network trouble** is reported as in the application: "Connecting / Reconnecting…" (your changes are kept and sent when the connection is back; the page also reconnects at once when the browser says the network is back or when you return to the tab), and, if the connection service cannot be reached (company or school network), the message with its three things to try and the **Copy the diagnostic** button. A web page cannot look at the network the way the application does, so the message is a little less precise.

**Differences from the application:** a browser guest only joins (it cannot host a session); the name and settings are not kept between visits except the name; images can be pasted (Ctrl+V) or dropped on the page and are sent to the host like those of an installed guest; no pen pressure.

---

## The MCP agent's cursor

If you use Inkboard with an AI agent through MCP (see [MCP.md](MCP.md)), the agent shows up as a participant named after the client (by default "Claude"): a coloured cursor glides to the objects it creates or changes, with a short trail and a halo on the touched objects. A look at the board (`render`, `describe_board`) is a discreet pulse.

- **On by default**; the setting is **Animate the agent** in the main menu (it is remembered). It affects display only, never the board, the undo history, or what the agent returns.
- **In a session**, the host's agent actions are also shown to the guests (each guest can switch the animation off for themselves).
- The agent is part of the **host's** application: it acts with the host's rights, its changes are attributed to the host (so the host's Ctrl+Z undoes them), and it cannot start, stop or configure a sharing session (there is no such MCP tool).
- A guest cannot drive the shared board with their own agent: the MCP server is not active while you are a guest.

---

## Known limits

- **Number of people**: 10 by default (you included), adjustable. Designed for about 20; protocol tests simulate 50 and 100 guests, but no real session of that size has been tried.
- **Images and icons for guests**: in the current version, a guest **cannot insert** images, icons, formulas, Mermaid diagrams, or paste/drop images (the buttons are greyed out, or a message says "Not available in a shared session yet"). Guests do see the host's images. Sending images from a guest is planned.
- **Browser guests**: see [the section above](#invite-someone-who-has-nothing-installed-join-from-a-browser) and [WEB.md](WEB.md). Chrome, Edge and Firefox are tested (WebKit, the engine of Safari, only approximately; real Safari and iOS are not tested); the page address to give depends on where the web page is published (not decided yet).
- **One board at a time**: you are either hosting or a guest, not both. Joining a session stops your own sharing.
- **The host must stay online.** If the host quits or loses the network for more than a minute or so, the session ends (guests can save a copy). Nothing is hosted for you.
- **Fonts**: the host's fonts are used for the layout of each computer. Text can wrap differently between two computers that do not have the same fonts. A common embedded font set is planned.
- **Starting and stopping** may freeze the window for up to 3 seconds while the connection service is contacted.
- **Resume after a long cut** can be slow: after a cut of one minute, the board may take half a minute more to come back once the network returns.
- Tested so far on a home network, a mobile network (5G) and a corporate network (where it failed, see below). Other networks, NAT types and browsers are not verified.

---

## Privacy and security

- **Offline by default.** Inkboard does not connect to anything until the host clicks **Start sharing** or a guest clicks **Join**. (When a connection fails, Inkboard may then contact the connection service's web address to diagnose why.)
- **End-to-end encryption.** The session is encrypted by the connection itself, **and** by a key that is only in the link. The relay, and anybody listening on the network, see encrypted bytes, not your board.
- **What the relay does see**: network metadata, in particular **IP addresses**, the times of the connections and the volume of data. That is inherent to any relay. By default Inkboard uses the free public relays run by the iroh project (n0), which are meant for development and hobby use and are rate-limited. For serious use, you can [run your own relay](RELAY.md).
- **Direct connections.** When two computers can reach each other directly (same network, VPN, or successful NAT traversal), the data does not go through the relay, and each side can see the other's IP address. The link also carries connection hints for the host's computer (addresses), another reason not to publish it.
- **You decide who enters**: every newcomer needs your approval, rights are enforced on your computer, and you can remove anybody. The name is a label, not an identity.
- **Guests have no access to your files.** Nothing from a guest can read or write files on your computer. Images sent by guests are size-limited and not decoded before display. A guest who exceeds the limits (message size, speed, quota) is warned, then removed.
- **No accounts, no history on a server, no tracking.** Inkboard has no server of its own for sessions.
- A guest's changes are applied to the host's board; if you want to keep the result, save it. If you want to keep your original, save a copy **before** sharing.

## Restricted networks (company, school)

Many company and school networks block unknown internet services. If you cannot join (or start a session that people can join), Inkboard shows a window titled **Cannot join the session**.

### What the message means

- *"Cannot reach the connection service. A firewall or a security gateway (often at work or at school) is probably blocking it."* The "connection service" is the relay, the server that lets two computers find each other. Your network does not let Inkboard reach it. If Inkboard recognises the gateway (for example Zscaler), the message names it.
- *"No connection: this computer does not seem to be connected to the Internet."* Check your Wi-Fi or cable.
- *"The connection service answers, but the host cannot be reached."* Your network is fine; the host closed the session or is offline. Ask them to check.

### The three things to try

When the network is likely to blame, the window lists three tips:

1. **Try another network**, for example your phone's connection sharing.
2. **Open the address of the service in your browser** (the window shows `https://<domain>`) and accept the warning page if there is one, then try again. Some gateways show a warning page ("unknown category") with a "Continue" button; a browser can click it, Inkboard cannot. *This remedy is not verified: in our test we saw the warning page, we did not confirm that accepting it unblocks Inkboard.*
3. **Ask your IT department to allow the domain** given in the window.

### The "Copy the diagnostic" button

The button **Copy the diagnostic** puts a short text (in English, so that your IT department can read it) in the clipboard. It contains: the Inkboard version, the connection service's domain and address, whether it was reachable, the HTTP status code, the `Server` header, the security gateway if one was recognised, and the error. It contains **nothing about your board, your name or the invitation link**. Paste it in your message to the IT department. **Try again** repeats the attempt; **Close** closes the window.

### What to tell the IT department

- The application needs outgoing HTTPS (port 443, web-socket connections) to the domain shown in the diagnostic.
- Everything is encrypted end to end; the relay cannot read the boards. It sees IP addresses and traffic volume.
- If that is too much for your policy, the company can **run its own relay** on a domain it controls and allows ([RELAY.md](RELAY.md)).
- Direct connections between computers on the same network or VPN do not need the relay (but need UDP to be allowed between them; not verified in a company network).

### The honest limit

The message makes the failure understandable; **it does not make the session work**. If your network refuses the relay's domain, the session will not work through it, and Inkboard does not try to get around your company's rules. Remaining options: another network (but beware that a security agent installed on your laptop may route everything through the company even on your phone's connection: not verified), a company-approved relay, or a direct connection on the company's local network or VPN (not verified in a corporate network). Also remember that sending company data to a third-party service can be against your company's policy: when in doubt, ask.

---

## Troubleshooting

| What you see | What it means / what to do |
|---|---|
| "Paste an invitation link first." | The link field is empty. |
| "This is not an Inkboard invitation link." / "The link is damaged or incomplete: ask for a new one." | The text pasted is not a complete link. Copy it again whole (it is long). |
| "This text is too long to be an invitation link." | You pasted something else. |
| "This invitation link is not valid any more: ask for a new one." | The host created new links, or ended the session. Ask for a new link. |
| "The session is full." | The host's maximum number of participants is reached. Ask the host to raise it. |
| "The host declined your request." | The host pressed **Decline**. |
| "You were removed from this session." / "You were removed from the session." | The host removed you. |
| "The host uses another version of Inkboard: update Inkboard (or reload the page)." | Host and guest have versions that cannot talk to each other. Update the older one. |
| "Waiting for the host's approval…" for long | The host has not accepted yet (the window has no time limit). Ask them to look at their Inkboard window. |
| "Cannot reach the connection service…" | See [Restricted networks](#restricted-networks-company-school). |
| "Reconnecting…" | A temporary cut. Wait; your changes are kept. |
| "The connection was lost for too long." | The cut lasted too long. Join again with the link (you may need the host to accept you again). |
| "You have read-only access: this change was refused." | You joined with the read-only link or the host switched you to read-only. |
| "You are sending changes too fast…" / "{name} went over a limit" | A guest sends too much too fast (a script, a stuck key). Repeated violations lead to removal. |
| Clicking an `inkboard://` link does nothing | Use the menu **Join a session…** and paste the link. |
| Connection is slow or the board takes long to arrive | The traffic goes through the relay, which is rate-limited when public. A relay of your own removes that limit. |
| The host cannot be joined on a local network | Allow Inkboard in the Windows firewall (private networks, including UDP) when it asks. Not verified on two real machines. |

## Frequently asked questions

**Do guests need an account or to install something?** No account. A guest can use the Inkboard application or just a recent browser (Chrome or Edge), with a web link.

**Does it cost anything?** No. By default it uses free public relays (limited speed, for development and hobby use). A relay of your own is optional.

**Where is my board stored?** On your computer only. The relay stores nothing and cannot read anything.

**Can several people write in the same text at the same time?** Not character by character. The last change received for an object wins.

**What happens to my own board when I join a session?** It is put aside and returned when you leave. Nothing of it is sent to the host.

**Can guests change my files?** No. They only change the board shown in the session; you decide whether to save it.

**Can I share without Internet?** On a local network or VPN, the computers can connect directly without the relay, in principle. This has not been verified on two real machines yet.

**How do I know who is there?** The green "●" counter in the title bar and the Participants list in the Share window (host); the names on the cursors (everybody).

**Can I share two boards at once?** No, one at a time.

**I clicked "Stop sharing", can people still reach me with the old link?** No. The link is invalid once the session ends; a new session gets new links.
